import 'dart:convert';
import 'dart:io';

Future<void> main() async {
  final client = HttpClient();
  final sessionId = 'debug-session-${DateTime.now().millisecondsSinceEpoch}';

  print('🔵 Starting Debug Session (ID: $sessionId)');

  // 1. First Turn
  print('\n[Turn 1] User: "Hello, my name is Debugger."');
  await _sendChat(
    client,
    sessionId,
    [
      {'role': 'user', 'content': 'Hello, my name is Debugger.'}
    ],
    isFresh: true,
  );

  // Check if session file exists
  final sessionFileSize = await _checkSessionFile(sessionId);

  if (sessionFileSize == null) {
    print('⚠️ Session file NOT found after Turn 1.');

    // Try calling Hibernate
    print('👉 Invoking Hibernate API explicitly...');
    await _hibernate(client, sessionId);

    final sizeAfterHibernate = await _checkSessionFile(sessionId);
    if (sizeAfterHibernate != null) {
      print(
          '✅ Session file created after Hibernate (Size: $sizeAfterHibernate bytes)');
    } else {
      print('❌ Session file STILL missing after Hibernate.');
    }
  } else {
    print('✅ Session file found immediately (Size: $sessionFileSize bytes)');
  }

  // 2. Second Turn
  print('\n[Turn 2] User: "What is my name?"');
  await _sendChat(
    client,
    sessionId,
    [
      {'role': 'user', 'content': 'What is my name?'}
    ],
    isFresh: false,
  );

  client.close();
}

Future<int?> _checkSessionFile(String sessionId) async {
  // We assume we are running in the workspace root or can find the sessions dir
  // The server is likely running in the same root.
  final file = File('sessions/$sessionId.json');
  if (await file.exists()) {
    return await file.length();
  }
  return null;
}

Future<void> _hibernate(HttpClient client, String sessionId) async {
  try {
    final request = await client.post('localhost', 8080, '/v1/chat/hibernate');
    request.headers.contentType = ContentType.json;
    request.write(jsonEncode({'user_id': sessionId}));
    final response = await request.close();
    final body = await response.transform(utf8.decoder).join();
    print('Hibernate Response: ${response.statusCode} $body');
  } catch (e) {
    print('Hibernate Request Error: $e');
  }
}

Future<void> _sendChat(
  HttpClient client,
  String sessionId,
  List<Map<String, dynamic>> messages, {
  bool isFresh = false,
}) async {
  try {
    final request =
        await client.post('localhost', 8080, '/v1/chat/completions');

    request.headers.contentType = ContentType.json;
    request.headers.add('X-Session-ID', sessionId);
    request.headers.add('X-Fresh-Session', isFresh.toString());

    final payload = {
      'model': 'test-model',
      'messages': messages,
      'stream': true
    };

    request.write(jsonEncode(payload));
    final response = await request.close();

    await response
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen((line) {
      if (line.startsWith('data: ') && !line.contains('[DONE]')) {
        try {
          final jsonStr = line.substring(6);
          final json = jsonDecode(jsonStr);
          if (json['choices'] != null && json['choices'].isNotEmpty) {
            final delta = json['choices'][0]['delta'];
            if (delta != null && delta['content'] != null) {
              stdout.write(delta['content']);
            }
          }
        } catch (e) {}
      }
    }).asFuture();

    print('');
  } catch (e) {
    print('Error: $e');
  }
}
