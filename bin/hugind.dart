import 'dart:io';
import 'package:args/command_runner.dart';
import 'package:hugind/commands/model_command.dart';
import 'package:hugind/commands/config_command.dart';
import 'package:hugind/commands/server_command.dart';

void main(List<String> arguments) async {
  if (arguments.contains('--version')) {
    const version = String.fromEnvironment('VERSION', defaultValue: 'dev');
    print('hugind version $version');
    exit(0);
  }

  final runner = CommandRunner('hugind', 'A simple command-line application.')
    ..addCommand(ModelCommand())
    ..addCommand(ConfigCommand())
    ..addCommand(ServerCommand());

  try {
    await runner.run(arguments);
  } catch (e) {
    print(e);
    exit(64); // Exit code for usage error
  }
}
