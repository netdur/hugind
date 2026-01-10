import 'dart:io';
import 'package:test/test.dart';
import 'package:hugind/server/config/server_config.dart';
import 'package:hugind/server/engine/engine_manager.dart';
import 'package:llama_cpp_dart/llama_cpp_dart.dart';

void main() {
  // Use the path from user's example
  const modelPath = '/Users/adel/Workspace/gguf/gemma-3-4b-it-q4_0.gguf';

  // Skip if model doesn't exist (local dev check)
  if (!File(modelPath).existsSync()) {
    print('SKIPPING: Model not found at $modelPath');
    return;
  }

  group('Engine Integration', () {
    late ServerConfig config;
    late EngineManager manager;

    setUp(() async {
      // Clean sessions
      final sessionDir = Directory('./sessions');
      if (sessionDir.existsSync()) sessionDir.deleteSync(recursive: true);
      sessionDir.createSync();

      manager = EngineManager.instance;

      // Mac Lib Path logic
      String? libPath;
      if (Platform.isMacOS) {
        libPath =
            "/Users/adel/Workspace/llama_cpp_dart/bin/MAC_ARM64/libllama.dylib";
      }

      config = ServerConfig(
        name: 'test_engine',
        host: 'localhost',
        port: 8080,
        libraryPath: libPath,
        concurrency: 2, // Test batching
        maxSlots: 4,
        timeoutSeconds: 30,
        systemPrompt: 'You are a test assistant.',
        embeddingsEnabled: false,
        sessionHome: './sessions',
        modelPath: modelPath,
        modelParams: ModelParams(),
        contextParams: ContextParams()..nCtx = 2048,
        samplerParams: SamplerParams(),
      );

      await manager.deploy(config);
    });

    tearDown(() async {
      await manager.dispose();
    });

    test('Single engine handles concurrent requests', () async {
      final engine = manager.getEngineForUser('user1');
      // Verify we only have 1 engine loaded
      expect(manager.loadedModels.length, 1);

      // Concurrent generation
      final stream1 = engine.generateStream(
          'user1', [Message(role: Role.user, content: 'Say "one"')]);
      final stream2 = engine.generateStream(
          'user2', [Message(role: Role.user, content: 'Say "two"')]);

      final results = await Future.wait([
        stream1.join(),
        stream2.join(),
      ]);

      print('Result 1: ${results[0]}');
      print('Result 2: ${results[1]}');

      expect(results[0], isNotEmpty);
      expect(results[1], isNotEmpty);

      // Free sessions to free up slots for next test
      await engine.freeSession('user1');
      await engine.freeSession('user2');
    });

    test('State persistence works', () async {
      final engine = manager.getEngineForUser('persist_user');

      // 1. Chat
      final s1 = engine.generateStream('persist_user',
          [Message(role: Role.user, content: 'My name is HugindTest.')]);
      await s1.drain();

      // 2. Hibernate
      await manager.hibernateSession('persist_user');

      // 3. New Engine instance (simulating server restart)
      // Since EngineManager is singleton, we dispose it and re-deploy?
      // Or just assume the engine re-loading session works if we start a new stream.
      // Ideally we'd fully restart the manager, but it's a singleton.
      // We can just rely on "isFreshSession=false" logic in generateStream if we passed previous messages?
      // Actually, to test persistence, we should verify the file exists first.

      expect(File('./sessions/persist_user.bin').existsSync(), isTrue);

      // 4. Continue chat (fresh session = false implies we want to append/load)
      // In a real server restart, isFreshSession would be true initially but we'd load if file exists?
      // The current logic in LlamaEngine.generateStream checks `!isFreshSession` to load.
      // Wait, if I restart the server, `isFreshSession` comes from the Client/ChatHandler.
      // If client says "I am continuing", isFreshSession is false.

      final s2 = engine.generateStream('persist_user',
          [Message(role: Role.user, content: 'What is my name?')],
          isFreshSession: false);

      final response = await s2.join();
      print('Persistence Response: $response');
      expect(response.toLowerCase(), contains('hugindtest'));
    });
  });
}
