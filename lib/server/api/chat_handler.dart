import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:shelf/shelf.dart';
import 'package:llama_cpp_dart/llama_cpp_dart.dart';

import '../engine/engine_manager.dart';
import '../engine/llama_engine.dart';

class ChatHandler {
  Future<Response> call(Request request) async {
    final tempFiles = <String>[];
    Stream<List<int>>? outboundStream;
    String userId = 'unknown';
    try {
      final bodyString = await request.readAsString();
      if (bodyString.isEmpty) return Response(400, body: 'Missing body');

      final json = jsonDecode(bodyString);
      if (json['messages'] == null)
        return Response(400, body: 'Missing messages');

      final rawMessages = json['messages'] as List;

      // Check for X-Session-ID header for stateful session management
      userId = request.headers['x-session-id'] ??
          request.headers['X-Session-ID'] ??
          json['user']?.toString() ??
          'default_session';

      // VISUAL LOG: Incoming
      print(
          '📩 Incoming Chat Request (User: $userId, Model: ${json['model']})');

      final messages = <Message>[];
      for (final m in rawMessages) {
        final content = m['content'];
        final imagesField = m['images'];
        final role = Role.fromString(m['role'] ?? 'user');

        final parsed = _parseContent(content, imagesField, tempFiles);
        messages.add(Message(
          role: role,
          content: parsed.content,
          images: parsed.images,
        ));
      }

      final engine = EngineManager.instance.getEngineForUser(userId);
      if (engine.config.embeddingsEnabled) {
        return Response(
          400,
          body: jsonEncode(
              {'error': 'This server is configured for embeddings only.'}),
          headers: {'content-type': 'application/json'},
        );
      }

      // Check for session continuity hint.
      // If 'false', the client asserts that it relies on previous context.
      // If the server cannot find it, it will abort instead of creating a fresh empty session.
      final isFreshSession =
          (request.headers['x-fresh-session'] ?? 'true').toLowerCase() ==
              'true';

      final tokenStream = engine.generateStream(userId, messages,
          isFreshSession: isFreshSession);

      // Create the SSE byte stream with [DONE] signal
      Stream<List<int>> sseStream() async* {
        try {
          await for (final token in tokenStream) {
            final chunk = {
              "id": "chatcmpl-${DateTime.now().millisecondsSinceEpoch}",
              "object": "chat.completion.chunk",
              "created": DateTime.now().millisecondsSinceEpoch ~/ 1000,
              "model": engine.config.name,
              "choices": [
                {
                  "index": 0,
                  "delta": {"content": token},
                  "finish_reason": null
                }
              ]
            };
            yield utf8.encode('data: ${jsonEncode(chunk)}\n\n');
          }

          // OpenAI Spec: Signal end of stream
          yield utf8.encode('data: [DONE]\n\n');
        } finally {
          _cleanupTempFiles(tempFiles);
        }
      }

      outboundStream = sseStream();

      return Response.ok(
        outboundStream,
        context: {"shelf.io.buffer_output": false},
        headers: {
          'Content-Type': 'text/event-stream',
          'Cache-Control': 'no-cache',
          'Connection': 'keep-alive',
        },
      );
    } catch (e, stack) {
      print('API Error: $e\n$stack');
      if (e is ArgumentError) {
        return Response(
          400,
          body: jsonEncode({'error': e.toString()}),
          headers: {'content-type': 'application/json'},
        );
      }
      if (e is ContextLostException) {
        print('   ⚠️ Context Lost for user $userId. Requesting refresh.');
        return Response(
          409, // Conflict
          body: jsonEncode({
            'error': 'Context lost',
            'code': 'context_lost',
            'message':
                'The server restart caused a loss of state. Please resend full history.'
          }),
          headers: {'content-type': 'application/json'},
        );
      }
      return Response.internalServerError(
          body: jsonEncode({'error': e.toString()}));
    } finally {
      // If we did NOT create a stream, we must clean up now.
      // If we DID create a stream, the stream handles cleanup.
      if (outboundStream == null) {
        print('   ⚠️ outboundStream is null, forcing cleanup immediately.');
        _cleanupTempFiles(tempFiles);
      } else {
        print('   ✅ outboundStream created, deferring cleanup.');
      }
    }
  }

  void _cleanupTempFiles(List<String> tempFiles) {
    if (tempFiles.isEmpty) return;
    print('   🧹 Cleaning up ${tempFiles.length} temp files...');
    for (final path in tempFiles) {
      try {
        if (File(path).existsSync()) {
          File(path).deleteSync();
          print('      - Deleted $path');
        } else {
          print('      - File not found: $path');
        }
      } catch (e) {
        print('      - Error deleting $path: $e');
      }
    }
  }

  _ParsedContent _parseContent(
      dynamic content, dynamic imagesField, List<String> tempFiles) {
    // OpenAI style: content can be a string, or a list of parts.
    // We also honor a legacy `images` array on the message map.
    final buffer = StringBuffer();
    final images = <String>[];

    void addImagePath(String path) {
      if (path.isEmpty) return;
      images.add(path);
    }

    if (content is String) {
      buffer.write(content);
    } else if (content is List) {
      for (final part in content) {
        if (part is Map && part['type'] == 'text') {
          buffer.write(part['text'] ?? '');
        } else if (part is Map && part['type'] == 'image_url') {
          final raw = part['image_url'];
          String? url;
          if (raw is String) {
            url = raw;
          } else if (raw is Map && raw['url'] != null) {
            url = raw['url'].toString();
          }
          if (url == null) {
            throw ArgumentError('Invalid image_url content');
          }
          final path = _materializeImage(url, tempFiles);
          print('   📷 Processed Attachment: $path');
          addImagePath(path);
        }
      }
    }

    if (imagesField is List) {
      for (final img in imagesField) {
        final path = _materializeImage(img.toString(), tempFiles);
        print('   📷 Processed Attachment (legacy): $path');
        addImagePath(path);
      }
    }

    return _ParsedContent(content: buffer.toString(), images: images);
  }

  String _materializeImage(String url, List<String> tempFiles) {
    // Support data URLs and local file paths/URIs. Remote HTTP fetch is not supported here.
    if (url.startsWith('data:')) {
      final commaIndex = url.indexOf(',');
      if (commaIndex == -1) {
        throw ArgumentError('Invalid data URL');
      }
      final base64Data = url.substring(commaIndex + 1);
      final bytes = base64.decode(base64Data);
      final tmp = File(
          '${Directory.systemTemp.path}/hugind_img_${DateTime.now().microsecondsSinceEpoch}.bin');
      tmp.writeAsBytesSync(bytes);
      tempFiles.add(tmp.path);
      return tmp.path;
    }

    if (url.startsWith('file://')) {
      final path = Uri.parse(url).toFilePath();
      if (!File(path).existsSync()) {
        throw ArgumentError('Image file not found: $path');
      }
      return path;
    }

    // Treat as plain local path.
    if (File(url).existsSync()) {
      return url;
    }

    throw ArgumentError(
        'Unsupported image reference. Use data: URLs or local file paths.');
  }
}

class _ParsedContent {
  final String content;
  final List<String> images;
  _ParsedContent({required this.content, required this.images});
}
