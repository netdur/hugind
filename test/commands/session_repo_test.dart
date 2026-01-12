import 'dart:convert';
import 'dart:io';

import 'package:hugind/commands/session_repo.dart';
import 'package:test/test.dart';
import 'package:path/path.dart' as p;

void main() {
  group('SessionRepo', () {
    late Directory tempDir;
    late SessionRepo repo;

    setUp(() {
      tempDir = Directory.systemTemp.createTempSync('hugind_test_');
      repo = SessionRepo(root: tempDir.path);
    });

    tearDown(() {
      if (tempDir.existsSync()) {
        try {
          tempDir.deleteSync(recursive: true);
        } catch (_) {}
      }
    });

    test('list() prefers stored title', () {
      final id = 'test-session';
      final file = File(p.join(tempDir.path, '$id.json'));

      file.writeAsStringSync(jsonEncode({
        'id': id,
        'model': 'test-model',
        'created': DateTime.now().toIso8601String(),
        'last_active': DateTime.now().toIso8601String(),
        'messages': [
          {'role': 'user', 'content': 'Hello world'}
        ],
        'title': 'Stored Title'
      }));

      final sessions = repo.list();
      expect(sessions.length, 1);
      expect(sessions.first.title, 'Stored Title');
    });

    test('list() falls back to first message', () {
      final id = 'test-session-2';
      final file = File(p.join(tempDir.path, '$id.json'));

      file.writeAsStringSync(jsonEncode({
        'id': id,
        'model': 'test-model',
        'created': DateTime.now().toIso8601String(),
        'last_active': DateTime.now().toIso8601String(),
        'messages': [
          {'role': 'user', 'content': 'First Message'}
        ],
        // No title
      }));

      final sessions = repo.list();
      expect(sessions.length, 1);
      expect(sessions.first.title, 'First Message');
    });
  });
}
