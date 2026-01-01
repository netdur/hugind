import 'dart:io';
import 'package:args/command_runner.dart';
import 'package:http/http.dart' as http;
import 'package:path/path.dart' as p;
import 'package:yaml/yaml.dart';

import '../agent/sandbox.dart';
import '../agent/capabilities.dart';

class AgentCommand extends Command {
  @override
  final String name = 'agent';
  @override
  final String description = 'Manage and run autonomous agents.';

  AgentCommand() {
    addSubcommand(AgentRunCommand());
    addSubcommand(AgentListCommand());
  }
}

class AgentListCommand extends Command {
  @override
  final String name = 'list';
  @override
  final String description = 'List available agents.';

  @override
  Future<void> run() async {
    final agentsDir = Directory(p.join(_configHome(), 'agents'));

    if (!agentsDir.existsSync()) {
      print('No agents found (directory does not exist: ${agentsDir.path})');
      return;
    }

    final entities = agentsDir.listSync();
    if (entities.isEmpty) {
      print('No agents installed.');
      return;
    }

    print('Available Agents:');
    print('-----------------');

    for (var entity in entities) {
      if (entity is Directory) {
        final name = p.basename(entity.path);
        final manifest = File(p.join(entity.path, 'agent.yaml'));
        String info = '';

        if (manifest.existsSync()) {
          try {
            final yaml = loadYaml(manifest.readAsStringSync());
            final version = yaml['version'];
            final desc = yaml['description'];
            if (version != null) info += ' (v$version)';
            if (desc != null) info += ' - $desc';
          } catch (_) {}
        }

        print('• $name$info');
      }
    }
    print('');
  }
}

class AgentRunCommand extends Command {
  @override
  final String name = 'run';
  @override
  final String description = 'Execute an agent.';

  @override
  Future<void> run() async {
    if (argResults!.rest.isEmpty) {
      print('Usage: hugind agent run <agent_name> [args...]');
      return;
    }

    final agentName = argResults!.rest.first;
    final args = argResults!.rest.skip(1).toList();

    // 1. Locate Agent
    Directory agentDir;
    final agentsDir = p.join(_configHome(), 'agents');

    if (agentName.contains(p.separator) || agentName.startsWith('.')) {
      // Treat as direct path
      agentDir = Directory(agentName);
      if (!agentDir.existsSync()) {
        print('❌ Agent path not found: "${agentDir.path}"');
        return;
      }
      // Resolve full path for clarity in logs
      if (!agentDir.isAbsolute) {
        agentDir = Directory(p.normalize(p.absolute(agentDir.path)));
      }
    } else {
      // Treat as installed agent name
      agentDir = Directory(p.join(agentsDir, agentName));
      if (!agentDir.existsSync()) {
        print('❌ Agent "$agentName" not found in $agentsDir');
        print(
            '   (To run a local agent, use path: hugind agent run ./$agentName)');
        return;
      }
    }

    final manifestFile = File(p.join(agentDir.path, 'agent.yaml'));
    if (!manifestFile.existsSync()) {
      print('❌ Agent manifest (agent.yaml) missing for "$agentName"');
      return;
    }

    // 2. Parse Manifest
    String? backendName;
    String entryPoint = 'main.drt';

    try {
      final content = await manifestFile.readAsString();
      final yaml = loadYaml(content);
      backendName = yaml['backend'] as String?;
      entryPoint = yaml['entry_point'] ?? 'main.drt';
    } catch (e) {
      print('❌ Failed to parse agent.yaml: $e');
      return;
    }

    if (backendName == null) {
      print('❌ Agent manifest must specify a "backend" (server config name).');
      return;
    }

    // 3. Resolve Backend
    final configPath = p.join(_configHome(), 'configs', '$backendName.yml');
    if (!File(configPath).existsSync()) {
      print('❌ Server config "$backendName" not found at $configPath');
      return;
    }

    // Parse Server Config to get port
    String host = '127.0.0.1';
    int port = 8080;

    try {
      final serverConfigContent = await File(configPath).readAsString();
      final serverYaml = loadYaml(serverConfigContent);
      final serverMap = serverYaml['server'] as Map?;
      host = serverMap?['host']?.toString() ?? host;
      port = int.tryParse(serverMap?['port']?.toString() ?? '') ?? port;
    } catch (e) {
      print('⚠️  Error reading server config, using defaults: $e');
    }

    final baseUrl = 'http://$host:$port';
    print('ℹ️  Agent "$agentName" connecting to $baseUrl ($backendName)...');

    // 4. Ping Server
    try {
      final resp = await http.get(Uri.parse('$baseUrl/health'));
      if (resp.statusCode != 200) {
        print('❌ Server health check failed: ${resp.statusCode}');
        return;
      }
    } catch (e) {
      print('❌ Could not connect to server at $baseUrl. Is it running?');
      print('   Run: hugind server start $backendName');
      return;
    }

    print('✅ Server is reachable.');

    // 5. Setup Capabilities
    final allowedPaths = <String>{
      Directory.current.path,
      agentDir.path,
    };

    // If the first arg looks like a directory, allow it for workDir usage.
    if (args.isNotEmpty) {
      final candidateDir = Directory(args.first);
      if (candidateDir.existsSync()) {
        allowedPaths.add(candidateDir.absolute.path);
      }
    }

    final sys = SysCapability(allowedPaths: allowedPaths.toList());
    final llm = LlmCapability(baseUrl);

    final sandbox = AgentSandbox(sys, llm);

    final scriptFile = File(p.join(agentDir.path, entryPoint));
    if (!scriptFile.existsSync()) {
      print('❌ Entry point $entryPoint not found.');
      return;
    }

    final sourceCode = await scriptFile.readAsString();

    print('🚀 Running Agent...');
    try {
      await sandbox.run(sourceCode, args);
      print('✅ Agent finished.');
    } catch (e) {
      print('❌ Agent runtime error: $e');
    }
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
