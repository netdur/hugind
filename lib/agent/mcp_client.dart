import 'dart:async';
import 'dart:convert';
import 'dart:io';

class McpClient {
  final String command;
  final List<String> args;

  Process? _process;
  final _pendingRequests = <int, Completer<dynamic>>{};
  int _seq = 0;

  // Stream to notify about incoming notifications (optional)
  // final _notifications = StreamController<Map<String, dynamic>>.broadcast();

  McpClient(this.command, this.args);

  Future<void> start() async {
    print('🔌 Starting MCP Server: $command ${args.join(' ')}');
    _process = await Process.start(command, args);

    // Handle stdout (JSON-RPC messages)
    _process!.stdout
        .transform(utf8.decoder)
        .transform(LineSplitter())
        .listen(_handleMessage, onError: (e) {
      print('❌ MCP Stdout Error ($command): $e');
    });

    // Handle stderr
    _process!.stderr.transform(utf8.decoder).listen((data) {
      if (data.trim().isNotEmpty) {
        print('⚠️ MCP Stderr ($command): $data');
      }
    });

    // Initialize
    await request('initialize', {
      'protocolVersion': '0.1.0',
      'capabilities': {},
      'clientInfo': {'name': 'hugind', 'version': '0.1.0'}
    });

    // Notify initialized
    notify('notifications/initialized', {});
  }

  Future<dynamic> request(String method, [Map<String, dynamic>? params]) async {
    final id = _seq++;
    final completer = Completer<dynamic>();
    _pendingRequests[id] = completer;

    final req = {
      'jsonrpc': '2.0',
      'id': id,
      'method': method,
      if (params != null) 'params': params
    };

    _send(req);
    return completer.future;
  }

  void notify(String method, [Map<String, dynamic>? params]) {
    final req = {
      'jsonrpc': '2.0',
      'method': method,
      if (params != null) 'params': params
    };
    _send(req);
  }

  void _send(Map<String, dynamic> json) {
    if (_process == null) throw Exception('MCP Client not started');

    final str = jsonEncode(json);
    _process!.stdin.writeln(str);
  }

  void _handleMessage(String line) {
    if (line.trim().isEmpty) return;

    try {
      final msg = jsonDecode(line);
      final id = msg['id'];

      if (id != null && _pendingRequests.containsKey(id)) {
        // It's a response
        if (msg.containsKey('error')) {
          _pendingRequests[id]!.completeError(msg['error']);
        } else {
          _pendingRequests[id]!.complete(msg['result']);
        }
        _pendingRequests.remove(id);
      } else {
        // Notification or request from server
        // For now, ignore
      }
    } catch (e) {
      print('❌ Failed to parse MCP message: $line\nError: $e');
    }
  }

  Future<void> stop() async {
    _process?.kill();
    _process = null;
  }
}
