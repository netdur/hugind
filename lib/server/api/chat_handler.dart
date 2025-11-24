import 'dart:async';
import 'dart:convert';

import 'package:shelf/shelf.dart';
import 'package:llama_cpp_dart/llama_cpp_dart.dart';

import '../engine/engine_manager.dart';

class ChatHandler {
  Future<Response> call(Request request) async {
    try {
      final bodyString = await request.readAsString();
      if (bodyString.isEmpty) return Response(400, body: 'Missing body');

      final json = jsonDecode(bodyString);
      if (json['messages'] == null)
        return Response(400, body: 'Missing messages');

      final rawMessages = json['messages'] as List;
      final userId = json['user']?.toString() ?? 'default_session';

      // VISUAL LOG: Incoming
      print(
          '📩 Incoming Chat Request (User: $userId, Model: ${json['model']})');

      final messages = rawMessages.map((m) {
        return Message(
          role: Role.fromString(m['role'] ?? 'user'),
          content: m['content'] ?? '',
        );
      }).toList();

      final engine = EngineManager.instance.getEngineForUser(userId);
      final tokenStream = engine.generateStream(userId, messages);

      // Create the SSE byte stream with [DONE] signal
      Stream<List<int>> sseStream() async* {
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
      }

      return Response.ok(
        sseStream(),
        context: {"shelf.io.buffer_output": false},
        headers: {
          'Content-Type': 'text/event-stream',
          'Cache-Control': 'no-cache',
          'Connection': 'keep-alive',
        },
      );
    } catch (e, stack) {
      print('API Error: $e\n$stack');
      return Response.internalServerError(
          body: jsonEncode({'error': e.toString()}));
    }
  }
}
