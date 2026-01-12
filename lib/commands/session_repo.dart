import 'dart:convert';
import 'dart:io';
import 'dart:math';
import 'package:path/path.dart' as p;

class SessionRepo {
  final String? _rootOverride;
  SessionRepo({String? root}) : _rootOverride = root;

  Directory get _dir {
    if (_rootOverride != null) return Directory(_rootOverride!);
    final env = Platform.environment;
    final home = env['HOME'] ?? env['USERPROFILE'] ?? '.';
    final d = Directory(p.join(home, '.hugind', 'chats'));
    if (!d.existsSync()) d.createSync(recursive: true);
    return d;
  }

  File _file(String id) => File(p.join(_dir.path, '$id.json'));

  bool exists(String id) => _file(id).existsSync();

  /// Create a new session and return the ID
  Future<String> create(String model) async {
    final id =
        'session-${DateTime.now().millisecondsSinceEpoch}-${Random().nextInt(999)}';
    final data = {
      'id': id,
      'model': model,
      'created': DateTime.now().toIso8601String(),
      'last_active': DateTime.now().toIso8601String(),
      'messages': []
    };
    await _file(id).writeAsString(jsonEncode(data));
    return id;
  }

  /// Load session data
  Future<Map<String, dynamic>> load(String id) async {
    if (!exists(id)) throw Exception("Session not found");
    return jsonDecode(await _file(id).readAsString());
  }

  /// Save session data
  Future<void> save(String id, Map<String, dynamic> data) async {
    data['last_active'] = DateTime.now().toIso8601String();
    await _file(id).writeAsString(jsonEncode(data));
  }

  /// Delete session
  Future<void> delete(String id) async {
    if (exists(id)) {
      await _file(id).delete();
    }
  }

  /// List all sessions for the Wizard
  List<SessionInfo> list() {
    if (!_dir.existsSync()) return [];
    final list = <SessionInfo>[];

    for (var f in _dir
        .listSync()
        .whereType<File>()
        .where((f) => f.path.endsWith('.json'))) {
      try {
        final json = jsonDecode(f.readAsStringSync());
        final msgs = json['messages'] as List;

        // Extract a title
        String title = json['title'] ?? "New Chat";

        // Fallback: If no explicit title, try to generate one from the first user message
        if (json['title'] == null) {
          final firstUser =
              msgs.firstWhere((m) => m['role'] == 'user', orElse: () => null);
          if (firstUser != null) {
            title =
                (firstUser['content'] as String).trim().replaceAll('\n', ' ');
            if (title.length > 30) title = "${title.substring(0, 30)}...";
          }
        }

        list.add(SessionInfo(
          id: json['id'],
          model: json['model'],
          title: title,
          lastActive: DateTime.parse(json['last_active'] ?? json['created']),
        ));
      } catch (_) {}
    }
    // Sort newest first
    list.sort((a, b) => b.lastActive.compareTo(a.lastActive));
    return list;
  }
}

class SessionInfo {
  final String id;
  final String model;
  final String title;
  final DateTime lastActive;
  SessionInfo(
      {required this.id,
      required this.model,
      required this.title,
      required this.lastActive});
}
