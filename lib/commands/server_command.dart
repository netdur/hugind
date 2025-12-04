import 'dart:async';
import 'dart:io';

import 'package:args/command_runner.dart';
import 'package:http/http.dart' as http;
import 'package:path/path.dart' as p;
import 'package:yaml/yaml.dart';
import 'package:llama_cpp_dart/llama_cpp_dart.dart';

import '../server/config/config_loader.dart';
import '../server/bootstrap.dart';
import '../server/config/server_config.dart';
import '../global_settings.dart';

class ServerCommand extends Command {
  @override
  final String name = 'server';
  @override
  final String description = 'Run and manage Hugind inference servers.';

  ServerCommand() {
    addSubcommand(ServerListCommand());
    addSubcommand(ServerStartCommand());
    addSubcommand(ServerStopCommand());
  }
}

// =============================================================================
// 1. LIST COMMAND
// =============================================================================
class ServerListCommand extends Command {
  @override
  final String name = 'list';
  @override
  final String description = 'List configs and check their running status.';

  @override
  Future<void> run() async {
    final configDir = Directory(p.join(_configHome(), 'configs'));
    if (!await configDir.exists()) {
      print('No configurations found in ${configDir.path}');
      return;
    }

    final files = configDir
        .listSync()
        .whereType<File>()
        .where((f) => f.path.endsWith('.yml') || f.path.endsWith('.yaml'))
        .toList();

    if (files.isEmpty) {
      print('No configurations found.');
      return;
    }

    print(
        '${"CONFIG".padRight(20)} ${"PORT".padRight(8)} ${"MODEL".padRight(30)} ${"STATUS"}');
    print('-' * 75);

    final futures = files.map(_checkServerStatus);
    final results = await Future.wait(futures);

    for (final row in results) {
      print(row);
    }
  }

  Future<String> _checkServerStatus(File configFile) async {
    final name = p.basenameWithoutExtension(configFile.path);
    String port = "----";
    String model = "Unknown";
    String status = "⚪️ Stopped";

    try {
      final content = await configFile.readAsString();
      final yaml = loadYaml(content);

      final serverConfig = yaml['server'];
      final modelConfig = yaml['model'];

      final host = serverConfig?['host'] ?? '127.0.0.1';
      final rawPort = serverConfig?['port'] ?? 8080;
      port = rawPort.toString();

      if (modelConfig != null && modelConfig['path'] != null) {
        model = p.basename(modelConfig['path'].toString());
        if (model.length > 28) model = '${model.substring(0, 25)}...';
      }

      final url = Uri.parse('http://$host:$port/health');

      try {
        final response =
            await http.get(url).timeout(const Duration(milliseconds: 500));
        if (response.statusCode == 200) {
          status = "🟢 Running";
        } else {
          status = "🔴 Error (${response.statusCode})";
        }
      } catch (_) {
        status = "⚪️ Stopped";
      }
    } catch (e) {
      status = "⚠️  Config Error";
    }

    return '${name.padRight(20)} ${port.padRight(8)} ${model.padRight(30)} $status';
  }
}

// =============================================================================
// 2. START COMMAND
// =============================================================================
class ServerStartCommand extends Command {
  @override
  final String name = 'start';
  @override
  final String description = 'Start a server instance in the foreground.';

  ServerStartCommand() {
    argParser.addOption('port', abbr: 'p', help: 'Override the config port');
    argParser.addOption('lib', help: 'Override path to libllama.so/dylib');
  }

  @override
  Future<void> run() async {
    if (argResults!.rest.isEmpty) {
      print('Usage: hugind server start <config_name>');
      return;
    }

    final configName = argResults!.rest.first;
    final configPath = p.join(_configHome(), 'configs', '$configName.yml');

    if (!File(configPath).existsSync()) {
      print('❌ Config "$configName" not found at $configPath');
      return;
    }

    print('🚀 Initializing Hugind Server ($configName)...');

    try {
      // 1. Load Configuration FIRST
      // We need this to see if the user defined 'library_path' in YAML
      var config = await ConfigLoader.load(configPath);

      // 2. Determine Library Path Priority:
      //    A. CLI Argument (--lib)
      //    B. Config YAML (server.library_path)
      //    C. Auto-Detection
      String? finalLibPath = argResults!['lib'];

      if (finalLibPath == null) {
        // Check config
        if (config.libraryPath != null) {
          if (File(config.libraryPath!).existsSync()) {
            finalLibPath = config.libraryPath;
          } else {
            print(
                '⚠️  Warning: Configured library path not found: ${config.libraryPath}');
            print('   → Attempting auto-detection...');
          }
        }
      }

      if (finalLibPath == null) {
        // Fallback to auto-detect
        finalLibPath = await _resolveLibraryPath();
      }

      // 3. Validate & Set
      if (finalLibPath == null || !File(finalLibPath).existsSync()) {
        print('❌ Fatal: Could not find libllama shared library.');
        print('   1. Set "library_path" in your config.yml');
        print('   2. Or provide path via --lib <path>');
        print('   3. Or ensure it exists in standard system paths.');
        exit(1);
      }

      Llama.libraryPath = finalLibPath;

      // 4. Apply Port Override
      if (argResults!['port'] != null) {
        final overridePort = int.tryParse(argResults!['port']);
        if (overridePort != null) {
          print('   → Overriding port: $overridePort');
          config = _overridePort(config, overridePort);
        }
      }

      // 5. Bootstrap
      await bootstrapServer(config);
    } catch (e) {
      // catch e, stacktrace if you want debugging
      print('\n❌ Fatal Error: $e');
      exit(1);
    }
  }

  // Helper to clone config with new port
  ServerConfig _overridePort(ServerConfig c, int newPort) {
    return ServerConfig(
      name: c.name,
      host: c.host,
      port: newPort,
      libraryPath: c.libraryPath,
      apiKey: c.apiKey,
      concurrency: c.concurrency,
      maxSlots: c.maxSlots,
      timeoutSeconds: c.timeoutSeconds,
      systemPrompt: c.systemPrompt,
      embeddingsEnabled: c.embeddingsEnabled,
      modelPath: c.modelPath,
      mmprojPath: c.mmprojPath,
      modelParams: c.modelParams,
      contextParams: c.contextParams,
      samplerParams: c.samplerParams,
    );
  }

  Future<String?> _resolveLibraryPath() async {
    // 1. Check Environment Variable
    final envPath = Platform.environment['LIBLLAMA_PATH'];
    if (envPath != null && File(envPath).existsSync()) return envPath;

    // 2. Check Global Settings
    final globalPath = await GlobalSettings.getLibraryPath();
    if (globalPath != null && File(globalPath).existsSync()) {
      return globalPath;
    }

    // 3. Auto-detection logic
    final scriptDir = p.dirname(Platform.script.toFilePath());
    final exeDir = p.dirname(Platform.resolvedExecutable);

    // Potential filenames
    final filenames = <String>[];
    if (Platform.isMacOS) {
      filenames.addAll(['libmtmd.dylib', 'libllama.dylib']);
    } else if (Platform.isWindows) {
      filenames.add('libllama.dll');
    } else {
      filenames.add('libllama.so');
    }

    // Potential directories
    final directories = [
      // Relative to executable (Homebrew Cellar / Dist)
      exeDir,
      p.join(exeDir, 'lib'),
      p.join(exeDir, '../lib'), // Common structure: bin/../lib

      // Dev / Script relative
      p.join(scriptDir, 'bin', 'MAC_ARM64'),
      p.join(Directory.current.path, 'bin', 'MAC_ARM64'),
      p.join(Directory.current.path, 'bin'),

      // System / Homebrew
      '/opt/homebrew/lib',
      '/usr/local/lib',
      '/usr/lib',
    ];

    for (final dir in directories) {
      for (final name in filenames) {
        final path = p.join(dir, name);
        if (File(path).existsSync()) return path;
      }
    }

    return null;
  }
}

// =============================================================================
// 3. STOP (HELPER) COMMAND
// =============================================================================
class ServerStopCommand extends Command {
  @override
  final String name = 'stop';
  @override
  final String description =
      'Print OS-specific commands to stop a running server for a config.';

  @override
  Future<void> run() async {
    if (argResults!.rest.isEmpty) {
      print('Usage: hugind server stop <config_name>');
      return;
    }

    final configName = argResults!.rest.first;
    final configPath = p.join(_configHome(), 'configs', '$configName.yml');

    if (!File(configPath).existsSync()) {
      print('❌ Config "$configName" not found at $configPath');
      return;
    }

    // Load just enough to read host/port.
    String host = '127.0.0.1';
    int port = 8080;
    try {
      final content = await File(configPath).readAsString();
      final yaml = loadYaml(content);
      final serverConfig = yaml['server'] as Map?;
      host = serverConfig?['host']?.toString() ?? host;
      port = int.tryParse(serverConfig?['port']?.toString() ?? '') ?? port;
    } catch (_) {
      // ignore parse errors, keep defaults
    }

    print('ℹ️  Target: $configName on $host:$port');

    final url = Uri.parse('http://$host:$port/health');
    try {
      final response =
          await http.get(url).timeout(const Duration(milliseconds: 500));
      if (response.statusCode == 200) {
        print('🟢 Detected a running server on $host:$port');
      } else {
        print('⚠️ Health check returned ${response.statusCode}; server may not be running.');
      }
    } catch (_) {
      print('⚪️ No response on $host:$port (may already be stopped).');
    }

    print('\nTo stop a process on port $port:');
    if (Platform.isMacOS) {
      print('  lsof -i :$port');
      print('  kill <PID>');
    } else if (Platform.isLinux) {
      print('  lsof -i :$port');
      print('  kill <PID>');
      print('  # or: fuser -k $port/tcp');
    } else if (Platform.isWindows) {
      print('  netstat -ano | findstr :$port');
      print('  taskkill /PID <PID> /F');
    } else {
      print('  (Unknown OS) Use a port/PID lister to find and kill the process.');
    }

    print('\nIf you started the server in this shell, Ctrl+C will stop it.');
  }
}

// --- Helpers ---

String _configHome() {
  final env = Platform.environment;
  if (Platform.isWindows) {
    final appData = env['APPDATA'];
    if (appData != null) return p.join(appData, 'hugind');
    return p.join(env['USERPROFILE'] ?? '.', '.hugind');
  }
  final xdg = env['XDG_CONFIG_HOME'];
  if (xdg != null && xdg.isNotEmpty) return p.join(xdg, 'hugind');
  return p.join(env['HOME'] ?? '.', '.hugind');
}
