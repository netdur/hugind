import 'dart:convert';
import 'dart:io';
import 'dart:async';

Future<void> main() async {
  final client = HttpClient();
  final sessionId = 'test-session-${DateTime.now().millisecondsSinceEpoch}';

  print('🔵 Starting Session Test (ID: $sessionId)');

  // 1. First Turn: Set context
  // We pass isFresh: true to signal a new session start
  print('\n[Turn 1] User: "Hello, my name is Adel."');
  await _sendChat(
    client,
    sessionId,
    [
      {'role': 'user', 'content': 'Hello, my name is Adel.'}
    ],
    isFresh: true,
  );

  // 2. Second Turn: Retrieve context
  // We pass isFresh: false to continue the existing session
  print('\n[Turn 2] User: "What is my name?"');
  await _sendChat(
    client,
    sessionId,
    [
      {'role': 'user', 'content': 'What is my name?'}
    ],
    isFresh: false,
  );

  client.close(); // Good practice to close the client when done
}

Future<void> _sendChat(
  HttpClient client,
  String sessionId,
  List<Map<String, dynamic>> messages, {
  bool isFresh = false, // Default to false
}) async {
  try {
    final request =
        await client.post('localhost', 8080, '/v1/chat/completions');

    // Set Headers
    request.headers.contentType = ContentType.json;
    request.headers.add('X-Session-ID', sessionId);
    // Add the specific header requested
    request.headers.add('X-Fresh-Session', isFresh.toString());

    final payload = {
      'model': 'test-model',
      'messages': messages,
      'stream': true
    };

    request.write(jsonEncode(payload));
    final response = await request.close();

    if (response.statusCode != 200) {
      print('HTTP Error: ${response.statusCode}');
      final body = await response.transform(utf8.decoder).join();
      print('Body: $body');
      return;
    }

    // Parse SSE (Server-Sent Events)
    // Using LineSplitter ensures we process complete lines even if packets are fragmented
    final done = Completer<void>();
    late final StreamSubscription<String> sub;

    sub = response
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen((line) async {
      if (line.startsWith('data: ')) {
        if (line.contains('[DONE]')) {
          if (!done.isCompleted) done.complete();
          await sub.cancel();
          return;
        }

        try {
          final jsonStr = line.substring(6); // Remove "data: "
          final json = jsonDecode(jsonStr);

          if (json['choices'] != null && json['choices'].isNotEmpty) {
            final delta = json['choices'][0]['delta'];
            if (delta != null && delta['content'] != null) {
              stdout.write(delta['content']);
            }
          }
        } catch (_) {
          // Suppress parsing errors for individual chunks
        }
      }
    }, onDone: () {
      if (!done.isCompleted) done.complete();
    }, onError: (_) {
      if (!done.isCompleted) done.complete();
    });

    await done.future;

    print(''); // Newline after stream finishes
  } catch (e) {
    print('Error: $e');
  }
}
