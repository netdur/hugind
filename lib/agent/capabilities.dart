import 'dart:io';
import 'dart:async';
import 'package:interact/interact.dart';
import 'package:http/http.dart' as http;
import 'dart:convert';
import 'package:path/path.dart' as p;
import 'mcp_client.dart';

class SysCapability {
  final List<String> allowedPaths;
  final bool shellAllowed;

  SysCapability({this.allowedPaths = const [], this.shellAllowed = false});

  Future<String> run(String executable, List<String> args,
      {String? workDir}) async {
    if (!shellAllowed) {
      // Check if this specific command is whitelisted?
      // For now, simpler boolean check as per initial plan
      return 'Permission denied: Shell execution is disabled for this agent.';
    }

    if (workDir != null) {
      if (!_isAllowed(workDir)) {
        return 'Access denied to path: $workDir. Allowed: $allowedPaths';
      }
    }

    try {
      final result =
          await Process.run(executable, args, workingDirectory: workDir);
      if (result.exitCode != 0) {
        return 'Command failed (exit ${result.exitCode}): ${result.stderr}';
      }
      return result.stdout.toString().trim();
    } catch (e) {
      return 'Execution failed: $e';
    }
  }

  Future<bool> confirm(String message) async {
    try {
      return Confirm(prompt: message, defaultValue: true).interact();
    } catch (_) {
      // Fallback if not interactive (fail closed)
      return false;
    }
  }

  void printMsg(String message) {
    print(message);
  }

  String readInput(String prompt) {
    stdout.write(prompt);
    return stdin.readLineSync() ?? '';
  }

  Future<String> readFile(String path) async {
    if (!_isAllowed(path)) {
      throw Exception('Access denied to path: $path. Allowed: $allowedPaths');
    }
    final file = File(path);
    if (!file.existsSync()) {
      throw Exception('File not found: $path');
    }
    return file.readAsString();
  }

  Future<bool> writeFile(String path, String contents) async {
    if (!_isAllowed(path)) {
      throw Exception('Access denied to path: $path. Allowed: $allowedPaths');
    }
    final file = File(path);
    await file.writeAsString(contents);
    return true;
  }

  Future<bool> exists(String path) async {
    if (!_isAllowed(path)) {
      return false;
    }
    return File(path).existsSync() || Directory(path).existsSync();
  }

  Future<bool> mkdir(String path, {bool recursive = true}) async {
    if (!_isAllowed(path)) {
      throw Exception('Access denied to path: $path. Allowed: $allowedPaths');
    }
    await Directory(path).create(recursive: recursive);
    return true;
  }

  bool _isAllowed(String path) {
    try {
      // 1. Resolve the allowed paths (roots) to their true physical paths
      final realAllowedPaths = allowedPaths
          .map((allowedPath) {
            final dir = Directory(allowedPath);
            return dir.existsSync() ? dir.resolveSymbolicLinksSync() : null;
          })
          .whereType<String>()
          .toList();

      // 2. Resolve the target path
      String realTargetPath;
      final f = File(path);
      final d = Directory(path);

      if (d.existsSync()) {
        realTargetPath = d.resolveSymbolicLinksSync();
      } else if (f.existsSync()) {
        realTargetPath = f.resolveSymbolicLinksSync();
      } else {
        // For non-existent files (e.g. creating output.txt), resolve the parent
        final parent = Directory(p.dirname(path));
        if (parent.existsSync()) {
          realTargetPath =
              p.join(parent.resolveSymbolicLinksSync(), p.basename(path));
        } else {
          // Fallback to absolute string if parent missing
          realTargetPath = p.normalize(p.absolute(path));
        }
      }

      // 3. Check against roots using proper path containment
      for (var root in realAllowedPaths) {
        if (root == realTargetPath || p.isWithin(root, realTargetPath)) {
          return true;
        }
      }
      return false;
    } catch (e) {
      print('Security check error for path $path: $e');
      return false; // Fail closed
    }
  }
}

class NetworkCapability {
  final List<String> allowedDomains;

  NetworkCapability({this.allowedDomains = const []});

  Future<String> fetch(String url) async {
    final uri = Uri.parse(url);

    if (uri.scheme != 'http' && uri.scheme != 'https') {
      throw Exception(
          'Permission denied: Only HTTP/HTTPS schemes are allowed.');
    }

    if (!_isAllowed(uri.host)) {
      throw Exception(
          'Permission denied: Network access to ${uri.host} is not allowed.');
    }

    // SSRF Protection: Resolve and check for private/loopback IPs
    try {
      final ips = await InternetAddress.lookup(uri.host);
      for (final ip in ips) {
        if (_isPrivate(ip)) {
          // Exception: Allow if host is explicitly 'localhost' or the literal IP
          // This prevents DNS rebinding attacks (external domain -> internal IP)
          if (uri.host == 'localhost' || uri.host == ip.address) {
            continue;
          }
          throw Exception(
              'Security Error: Access to private/loopback IP ${ip.address} denied (SSRF Protection).');
        }
      }
    } catch (e) {
      if (e.toString().contains('SSRF')) rethrow;
      // If lookup fails, http.get will likely fail too, but we let it proceed or fail safely?
      // Better to fail if we can't verify IP.
      throw Exception('DNS resolution failed or blocked: $e');
    }

    try {
      final response = await http.get(uri).timeout(const Duration(seconds: 30));
      return response.body;
    } catch (e) {
      throw Exception('Network request failed: $e');
    }
  }

  bool _isAllowed(String host) {
    for (var domain in allowedDomains) {
      if (host == domain || host.endsWith('.$domain')) {
        return true;
      }
    }
    return false;
  }

  bool _isPrivate(InternetAddress ip) {
    if (ip.isLoopback || ip.isLinkLocal) return true;

    // Manual check for IPv4 private ranges
    // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
    if (ip.type == InternetAddressType.IPv4) {
      final bytes = ip.rawAddress;
      if (bytes[0] == 10) return true;
      if (bytes[0] == 172 && bytes[1] >= 16 && bytes[1] <= 31) return true;
      if (bytes[0] == 192 && bytes[1] == 168) return true;
    }
    // IPv6 Unique Local Addresses (fc00::/7) ?
    // For now, focusing on standard private ranges.
    return false;
  }
}

class McpCapability {
  final Map<String, dynamic> serverConfigs;
  final List<String> requiredServers;
  final Map<String, McpClient> _clients = {};

  McpCapability(
      {this.serverConfigs = const {}, this.requiredServers = const []});

  Future<void> _ensureServer(String name) async {
    if (_clients.containsKey(name)) return;

    final config = serverConfigs[name];
    if (config == null) {
      throw Exception(
          'MCP Server "$name" is not configured in Global Settings.');
    }

    final cmd = config['command'] as String?;
    final args = (config['args'] as List?)?.cast<String>() ?? [];

    if (cmd == null) {
      throw Exception(
          'MCP Server "$name" is missing "command" in configuration.');
    }

    final client = McpClient(cmd, args);
    await client.start();
    _clients[name] = client;
  }

  Future<List<Map<String, dynamic>>> listTools() async {
    final allTools = <Map<String, dynamic>>[];

    // For now, we only connect to servers explicitly listed in dependencies
    // or we could iterate all configured servers.
    // Let's iterate required servers.
    for (var name in requiredServers) {
      try {
        await _ensureServer(name);
        final client = _clients[name]!;

        final result = await client.request('tools/list', {});
        final tools = (result['tools'] as List?) ?? [];

        for (var tool in tools) {
          // Tag tool with server name for routing
          tool['__server'] = name;
          allTools.add(Map<String, dynamic>.from(tool));
        }
      } catch (e) {
        print('⚠️ MCP Error ($name): $e');
      }
    }
    return allTools;
  }

  Future<dynamic> callTool(String name, Map<String, dynamic> args) async {
    // We need to find which server owns this tool.
    // Ideally, the caller passes the server name or we search.
    // The "tool call" protocol usually implies unique tool names or namespaced.
    // For now, we search all active clients.

    // Better strategy: iterate all required servers, ensure connected,
    // ask for tool execution. If failed (MethodNotFound), try next?
    // JSON-RPC requests usually throw if method not found, but 'tools/call' is the method.
    // The param 'name' specifies the tool.

    // Let's cache tool definitions on first listTools or initialization?
    // Or just try all of them sequentially until one works?

    for (var serverName in requiredServers) {
      await _ensureServer(serverName);
      final client = _clients[serverName]!;

      try {
        // We can check if this server has the tool if we kept the list.
        // Let's assume we do "tools/call" and it returns result.
        final result = await client
            .request('tools/call', {'name': name, 'arguments': args});
        return result;
      } catch (e) {
        // If error is "Tool not found", continue.
        // But JSON-RPC error codes are integers.
        // Let's just try matching the name against our known tool list if we had one.

        // To make this robust:
        // 1. Fetch tool list from this server.
        // 2. Check if name exists.
        // 3. If yes, call it.

        // This adds latency. Ideally we cache tool->server map.
        final list = await client.request('tools/list', {});
        final tools = (list['tools'] as List?) ?? [];
        final hasTool = tools.any((t) => t['name'] == name);

        if (hasTool) {
          final res = await client
              .request('tools/call', {'name': name, 'arguments': args});
          return res['content']; // MCP returns {content: [...], isError: bool}
        }
      }
    }
    throw Exception('Tool "$name" not found on any configured MCP server.');
  }

  Future<void> stopAll() async {
    for (var c in _clients.values) {
      await c.stop();
    }
    _clients.clear();
  }
}

class LlmCapability {
  final String baseUrl;

  LlmCapability(this.baseUrl);

  Future<String> chat(String prompt, {String? system}) async {
    final uri = Uri.parse('$baseUrl/v1/chat/completions');

    final messages = <Map<String, String>>[];
    if (system != null && system.isNotEmpty) {
      messages.add({'role': 'system', 'content': system});
    }
    messages.add({'role': 'user', 'content': prompt});

    final body = jsonEncode({
      'messages': messages,
      'temperature': 0.7,
      'stream': false,
    });

    try {
      final resp = await http
          .post(uri, body: body, headers: {'Content-Type': 'application/json'});
      if (resp.statusCode != 200) {
        throw Exception('LLM error ${resp.statusCode}: ${resp.body}');
      }

      try {
        final data = jsonDecode(resp.body);
        final content = data['choices']?[0]?['message']?['content'];
        if (content == null) return '';
        return content.toString();
      } catch (e) {
        // Fallback: Try parsing as SSE string
        final lines = resp.body.split('\n');
        final buffer = StringBuffer();
        for (var line in lines) {
          if (line.startsWith('data: ')) {
            final jsonStr = line.substring(6).trim();
            if (jsonStr == '[DONE]') continue;
            try {
              final chunk = jsonDecode(jsonStr);
              // Check for delta content
              final delta = chunk['choices']?[0]?['delta']?['content'];
              if (delta != null) {
                buffer.write(delta);
                continue;
              }
              // Check for message content (if non-streaming format sent in SSE wrapper)
              final content = chunk['choices']?[0]?['message']?['content'];
              if (content != null) {
                buffer.write(content);
              }
            } catch (_) {
              // ignore chunk parse error
            }
          }
        }
        final result = buffer.toString();
        if (result.isEmpty)
          throw e; // Rethrow original json error if no SSE content found
        return result;
      }
    } catch (e) {
      throw Exception('LLM request failed: $e');
    }
  }
}
