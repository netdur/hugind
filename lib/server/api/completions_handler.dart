import 'dart:convert';

import 'package:llama_cpp_dart/llama_cpp_dart.dart';
import 'package:shelf/shelf.dart';

import '../engine/engine_manager.dart';
import '../engine/llama_engine.dart';

class CompletionsHandler {
  Future<Response> call(Request request) async {
    try {
      final bodyString = await request.readAsString();
      if (bodyString.isEmpty) {
        return _badRequest('Missing body');
      }

      dynamic jsonBody;
      try {
        jsonBody = jsonDecode(bodyString);
      } catch (_) {
        return _badRequest('Invalid JSON body');
      }

      if (jsonBody is! Map || jsonBody['prompt'] == null) {
        return _badRequest('Missing "prompt"');
      }

      final rawPrompt = jsonBody['prompt'];
      final prompts = <String>[];

      if (rawPrompt is List) {
        for (final p in rawPrompt) {
          prompts.add(p.toString());
        }
      } else {
        prompts.add(rawPrompt.toString());
      }

      prompts.removeWhere((p) => p.trim().isEmpty);
      if (prompts.isEmpty) {
        return _badRequest('Prompt cannot be empty');
      }

      final stream = jsonBody['stream'] == true;
      if (stream && prompts.length != 1) {
        return _badRequest(
            'Streaming only supports a single prompt per request');
      }

      final userId = jsonBody['user']?.toString() ?? 'default_session';
      final engine = EngineManager.instance.getEngineForUser(userId);
      if (engine.config.embeddingsEnabled) {
        return Response(
          400,
          body: jsonEncode(
              {'error': 'This server is configured for embeddings only.'}),
          headers: {'content-type': 'application/json'},
        );
      }

      final model = engine.config.name;
      final id = 'cmpl-${DateTime.now().millisecondsSinceEpoch}';
      final created = DateTime.now().millisecondsSinceEpoch ~/ 1000;

      if (stream) {
        final tokenStream = engine.generateStream(
          userId,
          [Message(role: Role.user, content: prompts.first)],
        );

        Stream<List<int>> sseStream() async* {
          await for (final token in tokenStream) {
            final chunk = {
              'id': id,
              'object': 'text_completion.chunk',
              'created': created,
              'model': model,
              'choices': [
                {
                  'index': 0,
                  'text': token,
                  'logprobs': null,
                  'finish_reason': null,
                }
              ],
            };
            yield utf8.encode('data: ${jsonEncode(chunk)}\n\n');
          }
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
      }

      final choices = <Map<String, dynamic>>[];
      for (var i = 0; i < prompts.length; i++) {
        final text =
            await _collectCompletion(engine, userId, prompts[i]);
        choices.add({
          'index': i,
          'text': text,
          'logprobs': null,
          'finish_reason': 'stop',
        });
      }

      final response = {
        'id': id,
        'object': 'text_completion',
        'created': created,
        'model': model,
        'choices': choices,
        'usage': {'prompt_tokens': 0, 'completion_tokens': 0, 'total_tokens': 0}
      };

      return Response.ok(
        jsonEncode(response),
        headers: {'content-type': 'application/json'},
      );
    } catch (e, stack) {
      print('Completions error: $e\n$stack');
      return Response.internalServerError(
        body: jsonEncode({'error': e.toString()}),
        headers: {'content-type': 'application/json'},
      );
    }
  }

  Future<String> _collectCompletion(
    LlamaEngine engine,
    String userId,
    String prompt,
  ) async {
    final buffer = StringBuffer();
    final stream = engine.generateStream(
      userId,
      [Message(role: Role.user, content: prompt)],
    );

    await for (final token in stream) {
      buffer.write(token);
    }

    return buffer.toString();
  }

  Response _badRequest(String message) {
    return Response(
      400,
      body: jsonEncode({'error': message}),
      headers: {'content-type': 'application/json'},
    );
  }
}
