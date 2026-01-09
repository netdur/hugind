import 'dart:io';
import 'package:args/command_runner.dart';
import 'package:http/http.dart' as http;
import 'package:path/path.dart' as p;
import 'package:yaml/yaml.dart';

import '../agent/sandbox.dart';
import '../agent/capabilities.dart';
import '../global_settings.dart';

class AgentCommand extends Command {
  @override
  final String name = 'agent';
  @override
  final String description = 'Manage and run autonomous agents.';

  AgentCommand() {
    addSubcommand(AgentRunCommand());
    addSubcommand(AgentListCommand());
    addSubcommand(AgentInstallCommand());
  }
}

class AgentInstallCommand extends Command {
  @override
  final String name = 'install';
  @override
  final String description = 'Install an agent from a local directory or URL.';

  @override
  Future<void> run() async {
    if (argResults!.rest.isEmpty) {
      print('Usage: hugind agent install <path_to_agent_source>');
      return;
    }

    final sourcePath = argResults!.rest.first;
    final sourceDir = Directory(sourcePath);

    if (!sourceDir.existsSync()) {
      print('❌ Source directory not found: $sourcePath');
      // TODO: Support git URLs in the future
      return;
    }

    final manifestFile = File(p.join(sourceDir.path, 'agent.yaml'));
    if (!manifestFile.existsSync()) {
      print('❌ No agent.yaml found in $sourcePath');
      return;
    }

    try {
      final manifestContent = await manifestFile.readAsString();
      final yaml = loadYaml(manifestContent);

      final agentName = yaml['name'] as String?;
      if (agentName == null) {
        print('❌ Invalid manifest: "name" is required.');
        return;
      }

      print('📦 Installing \'$agentName\'...');

      // Parse permissions for display
      final permissions = yaml['permissions'] as Map?;
      if (permissions != null) {
        print('⚠️  PERMISSIONS REQUESTED:');

        final network = permissions['network'] as Map?;
        if (network != null) {
          final domains =
              (network['allowed_domains'] as List?)?.join(', ') ?? 'None';
          print('   • 🌐 Network: $domains');
        }

        final fs = permissions['filesystem'] as Map?;
        if (fs != null) {
          final read = fs['read'] == true;
          final write = fs['write'] == true;
          print(
              '   • 📂 Filesystem: Read=${read ? '✅' : '❌'}, Write=${write ? '✅' : '❌'}');
        }

        final mcp = yaml['dependencies']?['mcp'] as List?;
        if (mcp != null && mcp.isNotEmpty) {
          final names = mcp.map((e) => e['name']).join(', ');
          print('   • 🔌 MCP: Requires $names');
        }
      }

      // Confirmation
      // Note: We use the Interact package if available, or simple stdin
      stdout.write('\nDo you accept? [y/N] ');
      final input = stdin.readLineSync()?.toLowerCase();
      if (input != 'y' && input != 'yes') {
        print('❌ Installation cancelled.');
        return;
      }

      // Install
      final installDir = Directory(p.join(_configHome(), 'agents', agentName));
      if (installDir.existsSync()) {
        stdout.write('⚠️  Agent already exists. Overwrite? [y/N] ');
        final overwrite = stdin.readLineSync()?.toLowerCase();
        if (overwrite != 'y' && overwrite != 'yes') {
          print('❌ Installation cancelled.');
          return;
        }
        installDir.deleteSync(recursive: true);
      }

      installDir.createSync(recursive: true);

      // Copy files
      // Simple recursive copy
      await _copyDir(sourceDir, installDir);

      print('✅ Agent \'$agentName\' installed successfully!');
    } catch (e) {
      print('❌ Failed to install agent: $e');
    }
  }

  Future<void> _copyDir(Directory source, Directory dest) async {
    await for (final entity in source.list(recursive: false)) {
      final newPath = p.join(dest.path, p.basename(entity.path));
      if (entity is Directory) {
        await Directory(newPath).create();
        await _copyDir(entity, Directory(newPath));
      } else if (entity is File) {
        await entity.copy(newPath);
      }
    }
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

    // Parse Permissions
    var shellAllowed = false;
    var allowedDomains = <String>[];
    var requiredMcp = <String>[];

    try {
      // Re-read yaml to be safe/clean access (or I could cache it above,
      // but for now I'll just re-use the file reading logic or better, reuse the 'yaml' var if it was higher scope)
      // Wait, 'yaml' variable from step 2 is local to that try block.
      // I should probably move 'yaml' to outer scope or re-read.
      // To minimize diff, I'll re-read or just assume I can access it if I refactor.
      // Let's re-read for simplicity of this partial replacement.
      final content = await manifestFile.readAsString();
      final yaml = loadYaml(content);

      // Filesystem extras
      final fsConfig = yaml['permissions']?['filesystem'] as Map?;
      if (fsConfig != null) {
        final extras = fsConfig['allowed_paths'] as List?;
        if (extras != null) {
          allowedPaths.addAll(extras.cast<String>());
        }
      }

      // Network
      final netConfig = yaml['permissions']?['network'] as Map?;
      if (netConfig != null) {
        final domains = netConfig['allowed_domains'] as List?;
        if (domains != null) {
          allowedDomains.addAll(domains.cast<String>());
        }
      }

      // Shell
      final shellConfig = yaml['permissions']?['shell'] as Map?;
      if (shellConfig != null) {
        shellAllowed = shellConfig['allow'] == true;
      }

      // MCP Dependencies
      final mcpDeps = yaml['dependencies']?['mcp'] as List?;
      if (mcpDeps != null) {
        for (var dep in mcpDeps) {
          final name = dep['name'] as String?;
          if (name != null) requiredMcp.add(name);
        }
      }
    } catch (e) {
      print('⚠️  Failed to parse permissions from agent.yaml: $e');
    }

    // If the first arg looks like a directory, allow it for workDir usage.
    if (args.isNotEmpty) {
      final candidateDir = Directory(args.first);
      if (candidateDir.existsSync()) {
        allowedPaths.add(candidateDir.absolute.path);
      }
    }

    // Load MCP Settings
    final settings = await GlobalSettings.load();
    final mcpServers = (settings['mcp_servers'] as Map?) ?? {};

    final sys = SysCapability(
        allowedPaths: allowedPaths.toList(), shellAllowed: shellAllowed);
    final llm = LlmCapability(baseUrl);
    final net = NetworkCapability(allowedDomains: allowedDomains);
    final mcp = McpCapability(
        serverConfigs: mcpServers.cast<String, dynamic>(),
        requiredServers: requiredMcp);

    final sandbox = AgentSandbox(sys, llm, net, mcp);

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
