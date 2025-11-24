import 'dart:convert';
import 'package:shelf/shelf.dart';
import '../engine/engine_manager.dart';

class ModelsHandler {
  Response call(Request request) {
    // 1. Get active models
    final models = EngineManager.instance.loadedModels;

    // 2. Format as OpenAI JSON
    final response = {
      "object": "list",
      "data": models
          .map((name) => {
                "id": name,
                "object": "model",
                "created": DateTime.now().millisecondsSinceEpoch ~/ 1000,
                "owned_by": "hugind"
              })
          .toList()
    };

    return Response.ok(
      jsonEncode(response),
      headers: {'content-type': 'application/json'},
    );
  }
}
