import 'dart:convert';
import 'dart:io';

Future<void> main() async {
  final client = HttpClient();
  final sessionId = 'test-session-${DateTime.now().millisecondsSinceEpoch}';

  print('🔵 Starting Session Test (ID: $sessionId)');

  // 1. First Turn: Set context
  print('\n[Turn 1] User: "Hello, my name is Adel."');
  await _sendChat(client, sessionId, [
    {'role': 'user', 'content': 'Hello, my name is Adel.'}
  ]);

  // 2. Second Turn: Retrieve context
  print('\n[Turn 2] User: "What is my name?"');
  await _sendChat(client, sessionId, [
    {'role': 'user', 'content': 'What is my name?'}
  ]);
}

Future<void> _sendChat(HttpClient client, String sessionId,
    List<Map<String, dynamic>> messages) async {
  try {
    final request =
        await client.post('localhost', 8080, '/v1/chat/completions');
    request.headers.contentType = ContentType.json;
    request.headers.add('X-Session-ID', sessionId);

    final payload = {
      'model': 'test-model',
      'messages': messages,
      'stream':
          false // For simplicity in this test script, we wait for full response if server wasn't forcing SSE
    };

    // Note: The server forces SSE (text/event-stream) in the code I saw.
    // So we need to parse the stream.
    request.write(jsonEncode(payload));
    final response = await request.close();

    await response.transform(utf8.decoder).listen((data) {
      // Simple parsing of SSE data
      final lines = data.split('\n');
      for (final line in lines) {
        if (line.startsWith('data: ') && line != 'data: [DONE]') {
          try {
            final jsonStr = line.substring(6);
            final json = jsonDecode(jsonStr);
            if (json['choices'] != null && json['choices'].isNotEmpty) {
              final content = json['choices'][0]['delta']['content'];
              if (content != null) stdout.write(content);
            }
          } catch (_) {}
        }
      }
    }).asFuture();
    print(''); // Newline after stream
  } catch (e) {
    print('Error: $e');
  }
}
