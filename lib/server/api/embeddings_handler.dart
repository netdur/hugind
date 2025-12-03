import 'dart:convert';

import 'package:shelf/shelf.dart';

import '../engine/engine_manager.dart';

class EmbeddingsHandler {
  Future<Response> call(Request request) async {
    try {
      final bodyString = await request.readAsString();
      if (bodyString.isEmpty) {
        return Response(
          400,
          body: jsonEncode({'error': 'Missing body'}),
          headers: {'content-type': 'application/json'},
        );
      }

      final json = jsonDecode(bodyString);
      if (json is! Map || json['input'] == null) {
        return Response(
          400,
          body: jsonEncode({'error': 'Missing "input"'}),
          headers: {'content-type': 'application/json'},
        );
      }

      final rawInput = json['input'];
      final inputs = <String>[];

      if (rawInput is List) {
        for (final item in rawInput) {
          inputs.add(item.toString());
        }
      } else {
        inputs.add(rawInput.toString());
      }

      final userId = json['user']?.toString() ?? 'embed_request';
      final engine = EngineManager.instance.getEngineForUser(userId);

      if (!engine.config.embeddingsEnabled) {
        return Response(
          400,
          body: jsonEncode(
              {'error': 'Embeddings are disabled for this server config.'}),
          headers: {'content-type': 'application/json'},
        );
      }

      final data = <Map<String, dynamic>>[];
      for (var i = 0; i < inputs.length; i++) {
        final vector = await engine.embed(inputs[i]);
        data.add({
          'object': 'embedding',
          'index': i,
          'embedding': vector,
        });
      }

      final response = {
        'object': 'list',
        'model': engine.config.name,
        'data': data,
        'usage': {'prompt_tokens': 0, 'total_tokens': 0},
      };

      return Response.ok(
        jsonEncode(response),
        headers: {'content-type': 'application/json'},
      );
    } catch (e, stack) {
      print('Embeddings error: $e\n$stack');
      return Response.internalServerError(
        body: jsonEncode({'error': e.toString()}),
        headers: {'content-type': 'application/json'},
      );
    }
  }
}
