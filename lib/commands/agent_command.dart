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

      // Sanitize Agent Name (alphanumeric, dashes, underscores only)
      if (!RegExp(r'^[a-zA-Z0-9_-]+$').hasMatch(agentName)) {
        print(
            '❌ Invalid agent name "$agentName". Use only alphanumeric, "-", or "_".');
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
    var backendName = 'metal_unified';
    String? backendUrl;
    String? backendModel;
    var entryPoint = 'main.dart';
    YamlMap? agentYaml;

    try {
      final content = await manifestFile.readAsString();
      final loaded = loadYaml(content);
      if (loaded is YamlMap) {
        agentYaml = loaded;
        final backendConfig = agentYaml['backend'];
        if (backendConfig is String) {
          backendName = backendConfig;
        } else if (backendConfig is Map) {
          backendUrl = backendConfig['url']?.toString();
          backendModel = backendConfig['model']?.toString();
        }
        entryPoint = agentYaml['entry_point'] as String? ?? entryPoint;
        final requiredVersion = agentYaml['hugind_version']?.toString();
        if (requiredVersion != null && requiredVersion.isNotEmpty) {
          final currentVersion = _readHugindVersion();
          if (currentVersion == null) {
            print(
                '⚠️  Could not determine Hugind version to verify "$requiredVersion".');
          } else if (!_satisfiesVersion(currentVersion, requiredVersion)) {
            print(
                '❌ Agent requires Hugind $requiredVersion, but current version is $currentVersion.');
            return;
          }
        }
      }

      // Sanitize Entrypoint
      final resolvedEntry =
          p.normalize(p.join(agentDir.absolute.path, entryPoint));
      if (!p.isWithin(agentDir.absolute.path, resolvedEntry)) {
        print(
            '❌ Invalid entry_point "$entryPoint". Must be within the agent directory.');
        return;
      }
    } catch (e) {
      print('❌ Failed to parse agent.yaml: $e');
      return;
    }

    print('🚀 Launching agent: $agentName');
    if (backendUrl != null && backendUrl!.isNotEmpty) {
      print('   • Backend URL: $backendUrl');
      if (backendModel != null && backendModel!.isNotEmpty) {
        print('   • Model: $backendModel');
      }
    } else {
      print('   • Backend: $backendName');
    }
    print('   • Entry: $entryPoint');

    // 3. Resolve Backend
    String baseUrl;
    if (backendUrl != null && backendUrl!.isNotEmpty) {
      baseUrl = _normalizeBaseUrl(backendUrl!);
      print('ℹ️  Agent "$agentName" connecting to $baseUrl ...');
    } else {
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

      baseUrl = 'http://$host:$port';
      print('ℹ️  Agent "$agentName" connecting to $baseUrl ($backendName)...');
    }

    // 4. Ping Server
    try {
      final resp = await http.get(Uri.parse('${baseUrl}/health'));
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
    // Only allow the agent directory by default.
    // Allowing Directory.current is unsafe as it exposes the user's CWD (e.g. repo root with secrets).
    final allowedPaths = <String>{
      agentDir.path,
    };

    // Parse Permissions
    var shellAllowed = false;
    var allowedDomains = <String>[];
    var requiredMcp = <String>[];
    var optionalMcp = <String>[];
    var readAllowed = true;
    var writeAllowed = true;
    var networkAllowed = false;
    List<String> shellWhitelist = [];
    List<String> shellBlacklist = [];

    if (agentYaml != null) {
      // Filesystem extras
      final fsConfig = agentYaml['permissions']?['filesystem'] as Map?;
      if (fsConfig != null) {
        final extras = fsConfig['allowed_paths'] as List?;
        if (extras != null) {
          for (final raw in extras.cast<String>()) {
            allowedPaths.add(_resolveEnvPath(raw, agentDir));
          }
        }
        if (fsConfig['read'] == false) {
          readAllowed = false;
        }
        if (fsConfig['write'] == false) {
          writeAllowed = false;
        }
      }

      // Network
      final netConfig = agentYaml['permissions']?['network'] as Map?;
      if (netConfig != null) {
        networkAllowed = netConfig['allow'] == true;
        final domains = netConfig['allowed_domains'] as List?;
        if (domains != null) {
          allowedDomains.addAll(domains.cast<String>());
        }
      }

      // Shell
      final shellConfig = agentYaml['permissions']?['shell'] as Map?;
      if (shellConfig != null) {
        shellAllowed = shellConfig['allow'] == true;
        final whitelist = shellConfig['whitelist'] as List?;
        if (whitelist != null) {
          shellWhitelist = whitelist.cast<String>();
        }
        final blacklist = shellConfig['blacklist'] as List?;
        if (blacklist != null) {
          shellBlacklist = blacklist.cast<String>();
        }
        if (shellWhitelist.isNotEmpty && shellBlacklist.isNotEmpty) {
          print(
              '❌ Invalid manifest: shell whitelist and blacklist cannot both be set.');
          return;
        }
      }

      // MCP Dependencies
      final mcpDeps = agentYaml['dependencies']?['mcp'] as List?;
      if (mcpDeps != null) {
        for (var dep in mcpDeps) {
          final name = dep['name'] as String?;
          final required = dep['required'] == true;
          if (name != null && required) {
            requiredMcp.add(name);
          } else if (name != null) {
            optionalMcp.add(name);
          }
        }
      }

      // Env requirements
      final envDefs = agentYaml['env'] as List?;
      if (envDefs != null) {
        for (final entry in envDefs) {
          if (entry is! Map) continue;
          final name = entry['name']?.toString();
          final required = entry['required'] == true;
          if (required && (name == null || name.isEmpty)) {
            print('❌ Invalid manifest: env entry missing name.');
            return;
          }
          if (required && Platform.environment[name] == null) {
            print('❌ Missing required env var: $name');
            return;
          }
        }
      }
    }

    // Allow any arg that resolves to a directory (or its parent) for FS usage.
    for (final arg in args) {
      final candidateDir = Directory(arg);
      if (candidateDir.existsSync()) {
        allowedPaths.add(candidateDir.absolute.path);
        continue;
      }
      final candidateFile = File(arg);
      if (candidateFile.existsSync()) {
        final parent = candidateFile.parent;
        if (parent.existsSync()) {
          allowedPaths.add(parent.absolute.path);
        }
        continue;
      }
      if (arg.contains('/') || arg.startsWith('.')) {
        final parent = Directory(p.dirname(arg));
        if (parent.existsSync()) {
          allowedPaths.add(parent.absolute.path);
        }
      }
    }

    // Load MCP Settings
    final settings = await GlobalSettings.load();
    final mcpServers = (settings['mcp_servers'] as Map?) ?? {};
    final missingRequired = requiredMcp
        .where((name) => !mcpServers.containsKey(name))
        .toList();
    if (missingRequired.isNotEmpty) {
      print(
          '❌ Missing required MCP servers in settings: ${missingRequired.join(', ')}');
      return;
    }

    final sys = SysCapability(
        allowedPaths: allowedPaths.toList(),
        shellAllowed: shellAllowed,
        readAllowed: readAllowed,
        writeAllowed: writeAllowed,
        shellWhitelist: shellWhitelist,
        shellBlacklist: shellBlacklist);
    final llm = LlmCapability(baseUrl,
        model: backendUrl != null ? backendModel : backendName);
    final net = NetworkCapability(
        allowedDomains: allowedDomains, networkAllowed: networkAllowed);
    final mcp = McpCapability(
        serverConfigs: mcpServers.cast<String, dynamic>(),
        requiredServers: requiredMcp,
        optionalServers: optionalMcp);

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

String _normalizeBaseUrl(String rawUrl) {
  final trimmed = rawUrl.endsWith('/')
      ? rawUrl.substring(0, rawUrl.length - 1)
      : rawUrl;
  if (trimmed.endsWith('/v1')) {
    return trimmed.substring(0, trimmed.length - 3);
  }
  return trimmed;
}

String _resolveEnvPath(String rawPath, Directory agentDir) {
  var expanded = rawPath;
  final env = Platform.environment;
  if (expanded.startsWith('~/')) {
    final home = env['HOME'];
    if (home != null) {
      expanded = p.join(home, expanded.substring(2));
    }
  }
  expanded = expanded.replaceAllMapped(RegExp(r'\$\{([^}]+)\}'), (match) {
    final key = match.group(1);
    return env[key] ?? match.group(0)!;
  });
  expanded = expanded.replaceAllMapped(RegExp(r'\$([A-Za-z_][A-Za-z0-9_]*)'),
      (match) {
    final key = match.group(1);
    return env[key] ?? match.group(0)!;
  });

  if (!p.isAbsolute(expanded)) {
    return p.normalize(p.join(agentDir.path, expanded));
  }
  return p.normalize(expanded);
}

String? _readHugindVersion() {
  final pubspec = File(p.join(Directory.current.path, 'pubspec.yaml'));
  if (!pubspec.existsSync()) return null;
  try {
    final yaml = loadYaml(pubspec.readAsStringSync());
    return yaml['version']?.toString();
  } catch (_) {
    return null;
  }
}

bool _satisfiesVersion(String current, String constraint) {
  final match = RegExp(r'^(>=|<=|==|>|<)?\s*([0-9]+(?:\.[0-9]+){0,2})$')
      .firstMatch(constraint.trim());
  if (match == null) {
    return true;
  }
  final op = match.group(1) ?? '==';
  final target = match.group(2) ?? '';
  final cmp = _compareSemver(current, target);

  switch (op) {
    case '>':
      return cmp > 0;
    case '>=':
      return cmp >= 0;
    case '<':
      return cmp < 0;
    case '<=':
      return cmp <= 0;
    case '==':
    default:
      return cmp == 0;
  }
}

int _compareSemver(String a, String b) {
  final aParts = a.split('.').map((e) => int.tryParse(e) ?? 0).toList();
  final bParts = b.split('.').map((e) => int.tryParse(e) ?? 0).toList();
  while (aParts.length < 3) {
    aParts.add(0);
  }
  while (bParts.length < 3) {
    bParts.add(0);
  }
  for (var i = 0; i < 3; i++) {
    if (aParts[i] != bParts[i]) {
      return aParts[i].compareTo(bParts[i]);
    }
  }
  return 0;
}
