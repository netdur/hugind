import 'dart:io';
import 'package:test/test.dart';
import 'package:path/path.dart' as p;
import 'package:llama_cpp_dart/llama_cpp_dart.dart';
import 'package:hugind/server/config/config_loader.dart';

void main() {
  group('ConfigLoader', () {
    late Directory tempDir;

    setUp(() async {
      tempDir = await Directory.systemTemp.createTemp('hugind_test');

      // Mock HOME environment variable is tricky in Dart tests running in same process
      // So we might need to adjust ConfigLoader to accept a home path for testing
      // Or we just create the file in the real location if we are careful?
      // No, let's modify ConfigLoader to be testable.
    });

    tearDown(() async {
      await tempDir.delete(recursive: true);
    });

    test('loads valid config', () async {
      // Create dummy model file
      final modelFile = File(p.join(tempDir.path, 'model.gguf'));
      await modelFile.create();

      // Create config file
      final configContent = '''
model:
  path: ${modelFile.path}
  mmproj_path: ""
  gpu_layers: 33
  use_mlock: false
  use_mmap: true

context:
  size: 2048
  batch_size: 512
  flash_attention: true

sampling:
  temp: 0.7
  top_k: 50

server:
  port: 9090
''';
      final configFile = File(p.join(tempDir.path, 'test_config.yml'));
      await configFile.writeAsString(configContent);

      final config = await ConfigLoader.load(configFile.path);

      expect(config.name, equals('test_config'));
      expect(config.modelPath, equals(modelFile.path));
      expect(config.modelParams.nGpuLayers, equals(33));
      expect(config.contextParams.nCtx, equals(2048));
      expect(config.contextParams.flashAttention,
          equals(LlamaFlashAttnType.enabled));
      expect(config.samplerParams.temp, equals(0.7));
      expect(config.samplerParams.topK, equals(50));
      expect(config.port, equals(9090));
    });
  });
}
