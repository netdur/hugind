import 'dart:io';
import 'package:test/test.dart';
import 'package:hugind/global_settings.dart';
import 'package:path/path.dart' as p;

void main() {
  group('GlobalSettings', () {
    late Directory tempDir;
    late File settingsFile;

    setUp(() async {
      tempDir = await Directory.systemTemp.createTemp('hugind_settings_test_');
      settingsFile = File(p.join(tempDir.path, 'settings.yml'));
      GlobalSettings.testFile = settingsFile;
    });

    tearDown(() async {
      await tempDir.delete(recursive: true);
      GlobalSettings.testFile = null;
    });

    test('load returns empty if file does not exist', () async {
      final settings = await GlobalSettings.load();
      expect(settings, isEmpty);
    });

    test('set creates file if it does not exist', () async {
      expect(await settingsFile.exists(), isFalse);

      await GlobalSettings.set('foo', 'bar');

      expect(await settingsFile.exists(), isTrue);
      final content = await settingsFile.readAsString();
      expect(content, contains('foo: "bar"'));
    });

    test('set updates existing file', () async {
      await GlobalSettings.set('foo', 'bar');
      await GlobalSettings.set('baz', 'qux');

      final settings = await GlobalSettings.load();
      expect(settings['foo'], 'bar');
      expect(settings['baz'], 'qux');
    });
  });
}
