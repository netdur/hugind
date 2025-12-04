import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:shelf/shelf.dart';
import 'package:shelf/shelf_io.dart' as shelf_io;
import 'package:shelf_router/shelf_router.dart';

import 'config/server_config.dart';
import 'engine/engine_manager.dart';
import 'api/chat_handler.dart';
import 'api/models_handler.dart'; // <--- Import
import 'api/embeddings_handler.dart';
import 'api/completions_handler.dart';

Future<void> bootstrapServer(ServerConfig config) async {
  await _checkPortAvailability(config.host, config.port);

  print('   → Model: ${config.modelPath}');
  print(
      '   → Context: ${config.contextParams.nCtx} (Batch: ${config.contextParams.nBatch})');
  print(
      '   → Architecture: ${config.concurrency} Workers / ${config.maxSlots} Slots per worker');
  if (config.embeddingsEnabled) {
    print('   → Mode: embeddings-only (chat completions disabled)');
  }

  try {
    await EngineManager.instance.deploy(config);
  } catch (e) {
    print('\n❌ Failed to deploy model: $e');
    exit(1);
  }

  final app = Router();

  // 1. Health
  app.get('/health', (Request request) {
    return Response.ok(
        jsonEncode({'status': 'ok', 'model': config.name, 'active': true}),
        headers: {'content-type': 'application/json'});
  });

  // 2. Chat Completions
  if (config.embeddingsEnabled) {
    app.post('/v1/embeddings', EmbeddingsHandler());
  } else {
    app.post('/v1/chat/completions', ChatHandler());
    app.post('/v1/completions', CompletionsHandler());
  }

  // 3. List Models (NEW)
  app.get('/v1/models', ModelsHandler());

  final handler = Pipeline().addMiddleware(logRequests()).addHandler(app.call);

  final server = await shelf_io.serve(handler, config.host, config.port);

  print('\n✅ Server listening at http://${server.address.host}:${server.port}');
  print('   Local Health: http://127.0.0.1:${server.port}/health');
  print('   OpenAI URL:   http://127.0.0.1:${server.port}/v1');
  print('   Press Ctrl+C to stop.');

  ProcessSignal.sigint.watch().listen((_) async {
    print('\nStopping server...');
    await server.close();
    await EngineManager.instance.dispose();
    exit(0);
  });
}

Future<void> _checkPortAvailability(String host, int port) async {
  try {
    final server = await ServerSocket.bind(host, port);
    await server.close();
  } catch (e) {
    throw Exception(
        "Port $port is already in use. Please choose a different port in config.");
  }
}
