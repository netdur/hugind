import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:shelf/shelf.dart';
import 'package:llama_cpp_dart/llama_cpp_dart.dart';
import 'package:path/path.dart' as p;

import '../engine/engine_manager.dart';
import '../engine/llama_engine.dart';

class ChatHandler {
  static int _statelessCounter = 0;

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
      String userId;
      bool isFreshSession;

      final clientSessionId =
          request.headers['X-Session-ID'] ?? request.headers['x-session-id'];
      final freshHeader = request.headers['X-Fresh-Session'] ??
          request.headers['x-fresh-session'];

      if (clientSessionId != null && clientSessionId.isNotEmpty) {
        // CASE 1: Stateful / Custom ID provided by client.
        userId = clientSessionId;

        // DX Logic: Default to RESUMING (false), unless explicitly told to RESET (true).
        if (freshHeader != null && freshHeader.toLowerCase() == 'true') {
          isFreshSession = true; // Explicit reset requested
        } else {
          isFreshSession = false; // Implicit resume (Standard behavior)
        }
      } else {
        // CASE 2: Stateless / Anonymous (No ID provided).
        // Use a round-robin pool for stateless IDs.
        // FIX: Use sequential counter instead of Random to prevent collisions
        // under high concurrency (e.g. 2 requests picking same random slot).
        final n = _statelessCounter++;
        final slot = n % 32;
        userId = 'stateless-$slot';

        // SECURITY CRITICAL: Always force fresh for random slots to prevent
        // "Zombie Sessions" (loading a previous user's context).
        isFreshSession = true;
      }

      // VISUAL LOG: Incoming
      print(
          '📩 Incoming Chat Request (User: $userId, Model: ${json['model']}, Fresh: $isFreshSession)');

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

      final forkHeader =
          request.headers['X-Session-Fork'] ?? request.headers['x-session-fork'];
      if (forkHeader != null && forkHeader.trim().isNotEmpty) {
        if (!isFreshSession) {
          return Response(
            400,
            body: jsonEncode({
              'error': 'X-Session-Fork requires X-Fresh-Session: true'
            }),
            headers: {'content-type': 'application/json'},
          );
        }
        if (clientSessionId == null || clientSessionId.isEmpty) {
          return Response(
            400,
            body: jsonEncode(
                {'error': 'X-Session-Fork requires X-Session-ID'}),
            headers: {'content-type': 'application/json'},
          );
        }
        final templateName = forkHeader.trim();
        if (p.basename(templateName) != templateName) {
          return Response(
            400,
            body: jsonEncode({'error': 'Invalid template name'}),
            headers: {'content-type': 'application/json'},
          );
        }

        final sessionHome = engine.config.sessionHome;
        final templatePath = p.join(sessionHome, '$templateName.bin');
        final targetPath = p.join(sessionHome, '$userId.bin');

        if (!File(templatePath).existsSync()) {
          return Response(
            400,
            body: jsonEncode({'error': 'Template not found'}),
            headers: {'content-type': 'application/json'},
          );
        }

        try {
          File(templatePath).copySync(targetPath);
          print('   📌 Forked session from template: $templateName -> $userId');
        } catch (e) {
          return Response.internalServerError(
              body: jsonEncode({'error': 'Fork failed: $e'}));
        }
      }

      final tokenStream = engine.generateStream(userId, messages,
          isFreshSession: isFreshSession);

      // Create the SSE byte stream with [DONE] signal
      Stream<List<int>> sseStream() async* {
        final filter = _TokenStreamFilter();
        try {
          await for (final token in tokenStream) {
            final filtered = filter.process(token);
            if (filtered.done) {
              if (filtered.text.isNotEmpty) {
                final chunk = {
                  "id": "chatcmpl-${DateTime.now().millisecondsSinceEpoch}",
                  "object": "chat.completion.chunk",
                  "created": DateTime.now().millisecondsSinceEpoch ~/ 1000,
                  "model": engine.config.name,
                  "choices": [
                    {
                      "index": 0,
                      "delta": {"content": filtered.text},
                      "finish_reason": null
                    }
                  ]
                };
                yield utf8.encode('data: ${jsonEncode(chunk)}\n\n');
              }
              break;
            }
            if (filtered.text.isEmpty) continue;
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

          final remainder = filter.flush();
          if (remainder.isNotEmpty) {
            final chunk = {
              "id": "chatcmpl-${DateTime.now().millisecondsSinceEpoch}",
              "object": "chat.completion.chunk",
              "created": DateTime.now().millisecondsSinceEpoch ~/ 1000,
              "model": engine.config.name,
              "choices": [
                {
                  "index": 0,
                  "delta": {"content": remainder},
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

class _TokenChunk {
  final String text;
  final bool done;
  _TokenChunk(this.text, this.done);
}

class _TokenStreamFilter {
  final List<String> stopSequences = ['[EOS]', '<|endoftext|>', '<|im_end|>'];
  String _buffer = '';

  _TokenChunk process(String chunk) {
    _buffer += chunk;

    for (final stop in stopSequences) {
      if (_buffer.contains(stop)) {
        final index = _buffer.indexOf(stop);
        final text = _buffer.substring(0, index);
        _buffer = _buffer.substring(index + stop.length);
        return _TokenChunk(text, true);
      }
    }

    if (_buffer.length > 50) {
      final safeLen = _buffer.length - 20;
      final text = _buffer.substring(0, safeLen);
      _buffer = _buffer.substring(safeLen);
      return _TokenChunk(text, false);
    }

    final partialKeep = _partialStopSuffixLength();
    if (partialKeep > 0) {
      final splitPoint = _buffer.length - partialKeep;
      final text = _buffer.substring(0, splitPoint);
      _buffer = _buffer.substring(splitPoint);
      return _TokenChunk(text, false);
    }

    final text = _buffer;
    _buffer = '';
    return _TokenChunk(text, false);
  }

  String flush() {
    final text = _buffer;
    _buffer = '';
    for (final stop in stopSequences) {
      if (text == stop) return '';
    }
    return text;
  }

  int _partialStopSuffixLength() {
    var keepLength = 0;
    for (final stop in stopSequences) {
      for (var i = 1; i < stop.length; i++) {
        if (i > _buffer.length) break;
        final suffix = _buffer.substring(_buffer.length - i);
        if (stop.startsWith(suffix) && i > keepLength) keepLength = i;
      }
    }
    return keepLength;
  }
}
