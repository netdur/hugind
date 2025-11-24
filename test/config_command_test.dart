import 'package:test/test.dart';
import 'package:hugind/commands/config_templates.dart';
import 'package:yaml/yaml.dart';

void main() {
  group('ConfigTemplates', () {
    test('configYml should be valid YAML', () {
      expect(() => loadYaml(ConfigTemplates.configYml), returnsNormally);
      final yaml = loadYaml(ConfigTemplates.configYml);
      expect(yaml['model'], isNotNull);
      expect(yaml['server'], isNotNull);
    });

    test('metalUnified should be valid YAML', () {
      expect(() => loadYaml(ConfigTemplates.metalUnified), returnsNormally);
      final yaml = loadYaml(ConfigTemplates.metalUnified);
      expect(yaml['device']['devices'], contains('metal'));
    });

    test('cudaDedicated should be valid YAML', () {
      expect(() => loadYaml(ConfigTemplates.cudaDedicated), returnsNormally);
      final yaml = loadYaml(ConfigTemplates.cudaDedicated);
      expect(yaml['device']['devices'], contains('cuda:0'));
    });

    test('cpuOnly should be valid YAML', () {
      expect(() => loadYaml(ConfigTemplates.cpuOnly), returnsNormally);
      final yaml = loadYaml(ConfigTemplates.cpuOnly);
      expect(yaml['device']['gpu_layers'], equals(0));
    });

    test('Templates should contain placeholders', () {
      expect(ConfigTemplates.cpuOnly, contains('{{ physical_cores - 1 }}'));
      expect(
          ConfigTemplates.metalUnified, contains('{{ physical_cores - 2 }}'));
    });
  });
}
