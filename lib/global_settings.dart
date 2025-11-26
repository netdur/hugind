import 'dart:io';
import 'package:path/path.dart' as p;
import 'package:yaml/yaml.dart';

class GlobalSettings {
  static File? _testFile;

  static set testFile(File? file) => _testFile = file;

  static File get _file {
    if (_testFile != null) return _testFile!;
    final env = Platform.environment;
    final home = env['HOME'] ?? env['USERPROFILE'] ?? '.';
    return File(p.join(home, '.hugind', 'settings.yml'));
  }

  static Future<Map<String, dynamic>> load() async {
    if (!await _file.exists()) return {};
    try {
      final content = await _file.readAsString();
      final yaml = loadYaml(content);
      if (yaml is Map) return Map<String, dynamic>.from(yaml);
    } catch (e) {
      print('Warning: Failed to load global settings: $e');
    }
    return {};
  }

  static Future<void> set(String key, String value) async {
    final current = await load();
    current[key] = value;

    final buffer = StringBuffer();
    buffer.writeln('# Hugind Global Settings');
    current.forEach((k, v) => buffer.writeln('$k: "$v"'));

    if (!await _file.exists()) {
      await _file.create(recursive: true);
    }
    await _file.writeAsString(buffer.toString());
  }

  static Future<String?> getLibraryPath() async {
    final data = await load();
    return data['library_path']?.toString();
  }

  static Future<String?> getHfToken() async {
    final data = await load();
    return data['hf_token']?.toString();
  }
}
