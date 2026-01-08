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
import 'api/hibernate_handler.dart';

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
    app.post('/v1/chat/hibernate', HibernateHandler());
    app.post('/v1/completions', CompletionsHandler());
  }

  // 3. List Models (NEW)
  app.get('/v1/models', ModelsHandler());

  var pipeline = Pipeline().addMiddleware(logRequests());

  // Auth Middleware
  if (config.apiKey != null && config.apiKey!.isNotEmpty) {
    pipeline = pipeline.addMiddleware((innerHandler) {
      return (request) {
        // Skip auth for health
        if (request.url.path == 'health') return innerHandler(request);

        final auth = request.headers['Authorization'];
        String? token;
        if (auth != null && auth.startsWith('Bearer ')) {
          token = auth.substring(7);
        } else if (auth != null && auth.startsWith('Basic ')) {
          // Some clients (like older OpenAI libs) might rely on Basic?
          // Standard is Bearer for OpenAI. Let's support just key checks.
          // Actually, some clients send API key as a username with empty password in Basic auth.
          // Let's stick to Bearer for now, as that is the OpenAI standard key transport.
        }

        // Also check if provided as query param (unlikely for OpenAI but handy)
        if (token == null && request.url.queryParameters.containsKey('key')) {
          token = request.url.queryParameters['key'];
        }

        if (token != config.apiKey) {
          return Response.forbidden(
              jsonEncode({
                'error': {'message': 'Invalid API Key', 'type': 'auth_error'}
              }),
              headers: {'content-type': 'application/json'});
        }

        return innerHandler(request);
      };
    });
    print('   🔒 Auth Enabled: API Key required (Bearer <token>)');
  }

  final handler = pipeline.addHandler(app.call);

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
