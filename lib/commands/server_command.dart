import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:args/command_runner.dart';
import 'package:http/http.dart' as http;
import 'package:path/path.dart' as p;
import 'package:yaml/yaml.dart';
import 'package:interact/interact.dart';
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

  @override
  Future<void> run() async {
    // If we are here, no subcommand was passed
    await _runWizard();
  }

  Future<void> _runWizard() async {
    final configDir = Directory(p.join(_configHome(), 'configs'));
    if (!await configDir.exists()) {
      print('No configurations found in ${configDir.path}.');
      // Create new?
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

    final configNames =
        files.map((f) => p.basenameWithoutExtension(f.path)).toList();

    // Check status of each (for better UI)
    print('🦅 Hugind Server Manager');

    // We can show status in the selection list?
    // "model-a (Running)", "model-b"
    // Loading status might take time. Let's just list names for now or do a quick check.

    final selection = Select(
      prompt: 'Select a server config to start:',
      options: configNames,
    ).interact();

    final selectedConfig = configNames[selection];

    // Ask for port override?
    // final port = Input(prompt: "Port (default from config):").interact();
    // For now, straight start.

    await startServerSequence(selectedConfig);
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

      try {
        final modelsUrl = Uri.parse('http://$host:$port/v1/models');
        final response =
            await http.get(modelsUrl).timeout(const Duration(milliseconds: 500));
        if (response.statusCode == 200) {
          final decoded = jsonDecode(response.body);
          final data = decoded is Map ? decoded['data'] : null;
          final ids = <String>[];
          if (data is List) {
            for (final entry in data) {
              if (entry is Map && entry['id'] is String) {
                ids.add(entry['id'] as String);
              }
            }
          }

          if (ids.contains(name)) {
            status = "🟢 Running";
          } else {
            status = "⚪️ Stopped";
          }
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
    final portOverride =
        argResults!['port'] != null ? int.tryParse(argResults!['port']) : null;

    await startServerSequence(configName,
        portOverride: portOverride, libOverride: argResults!['lib']);
  }

  // Helpers moved to top level
}

// =============================================================================
// SHARED LOGIC
// =============================================================================

Future<void> startServerSequence(String configName,
    {int? portOverride, String? libOverride}) async {
  final configPath = p.join(_configHome(), 'configs', '$configName.yml');

  if (!File(configPath).existsSync()) {
    print('❌ Config "$configName" not found at $configPath');
    return;
  }

  print('🚀 Initializing Hugind Server ($configName)...');

  try {
    // 1. Load Configuration FIRST
    var config = await ConfigLoader.load(configPath);

    // 2. Determine Library Path Priority
    String? finalLibPath = libOverride;

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
    if (portOverride != null) {
      print('   → Overriding port: $portOverride');
      config = _overridePort(config, portOverride);
    }

    // 5. Bootstrap
    await bootstrapServer(config);
  } catch (e) {
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
    sessionHome: c.sessionHome,
    modelPath: c.modelPath,
    mmprojPath: c.mmprojPath,
    modelParams: c.modelParams,
    contextParams: c.contextParams,
    samplerParams: c.samplerParams,
    chatFormat: c.chatFormat,
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

    final modelIds = await _fetchModelIds(host, port);
    if (modelIds.isNotEmpty) {
      if (modelIds.contains(configName)) {
        print('🟢 Detected a running server on $host:$port');
      } else {
        print(
            '⚠️ Server on $host:$port is running different model(s): ${modelIds.join(', ')}');
      }
    } else {
      print('⚪️ No response on $host:$port (may already be stopped).');
    }

    final pids = await _pidsForPort(port);
    if (pids.isEmpty) {
      print('⚪️ No listening process found on port $port.');
      return;
    }

    print('🧹 Stopping process(es) on port $port: ${pids.join(', ')}');
    for (final pid in pids) {
      final stopped = await _killPid(pid);
      if (stopped) {
        print('  ✓ Killed PID $pid');
      } else {
        print('  ⚠️ Failed to kill PID $pid (insufficient permissions?)');
      }
    }
  }
}

// --- Helpers ---

Future<List<String>> _fetchModelIds(String host, int port) async {
  try {
    final modelsUrl = Uri.parse('http://$host:$port/v1/models');
    final response =
        await http.get(modelsUrl).timeout(const Duration(milliseconds: 500));
    if (response.statusCode != 200) return [];
    final decoded = jsonDecode(response.body);
    final data = decoded is Map ? decoded['data'] : null;
    if (data is! List) return [];
    final ids = <String>[];
    for (final entry in data) {
      if (entry is Map && entry['id'] is String) {
        ids.add(entry['id'] as String);
      }
    }
    return ids;
  } catch (_) {
    return [];
  }
}

Future<List<int>> _pidsForPort(int port) async {
  if (Platform.isWindows) {
    return _pidsForPortWindows(port);
  }
  return _pidsForPortUnix(port);
}

Future<List<int>> _pidsForPortUnix(int port) async {
  try {
    final result = await Process.run(
      'lsof',
      ['-n', '-iTCP:$port', '-sTCP:LISTEN', '-t'],
    );
    if (result.exitCode != 0) return [];
    final lines = result.stdout.toString().split('\n');
    final pids = <int>[];
    for (final line in lines) {
      final trimmed = line.trim();
      if (trimmed.isEmpty) continue;
      final pid = int.tryParse(trimmed);
      if (pid != null) pids.add(pid);
    }
    return pids;
  } catch (_) {
    return [];
  }
}

Future<List<int>> _pidsForPortWindows(int port) async {
  try {
    final result = await Process.run('netstat', ['-ano']);
    if (result.exitCode != 0) return [];
    final lines = result.stdout.toString().split('\n');
    final pids = <int>[];
    final matcher = RegExp(
        r'^\s*TCP\s+\S+:' + port.toString() + r'\s+\S+\s+LISTENING\s+(\d+)\s*$');
    for (final line in lines) {
      final match = matcher.firstMatch(line);
      if (match != null) {
        final pid = int.tryParse(match.group(1) ?? '');
        if (pid != null) pids.add(pid);
      }
    }
    return pids;
  } catch (_) {
    return [];
  }
}

Future<bool> _killPid(int pid) async {
  if (Platform.isWindows) {
    try {
      final result =
          await Process.run('taskkill', ['/PID', '$pid', '/F']);
      return result.exitCode == 0;
    } catch (_) {
      return false;
    }
  }

  try {
    final ok = Process.killPid(pid, ProcessSignal.sigterm);
    if (ok) return true;
  } catch (_) {
    // fall through to SIGKILL
  }

  try {
    final result = await Process.run('kill', ['-9', '$pid']);
    return result.exitCode == 0;
  } catch (_) {
    return false;
  }
}

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
