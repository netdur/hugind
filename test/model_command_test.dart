import 'dart:io';
import 'package:args/command_runner.dart';
import 'package:hugind/commands/model_command.dart';
import 'package:hugind/repo_manager.dart';
import 'package:test/test.dart';

void main() {
  late Directory tempDir;
  late RepoManager manager;
  late CommandRunner runner;

  setUp(() async {
    tempDir = await Directory.systemTemp.createTemp('hugind_test_');
    manager = RepoManager(rootDir: tempDir);
    runner = CommandRunner('hugind', 'Test runner')
      ..addCommand(ModelCommand(manager: manager));
  });

  tearDown(() async {
    await tempDir.delete(recursive: true);
  });

  test('list command - empty', () async {
    await runner.run(['model', 'list']);
  });

  // Note: Interactive commands are hard to test with standard unit tests.
  // We would need to mock the Interact components or the RepoManager calls.
  // For now, we verify the structure and basic execution.
}
