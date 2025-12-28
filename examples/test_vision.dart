import 'dart:convert';
import 'dart:io';

Future<void> main(List<String> args) async {
  if (args.isEmpty) {
    print('Usage: dart examples/test_vision.dart <path/to/image.jpg>');
    exit(1);
  }

  final imagePath = args[0];
  if (!File(imagePath).existsSync()) {
    print('Error: File not found: $imagePath');
    exit(1);
  }

  final client = HttpClient();
  final sessionId = 'vision-test-${DateTime.now().millisecondsSinceEpoch}';

  print('🔵 Starting Vision Test');
  print('   Target Image: $imagePath');

  // Convert image to base64 data URI to simulate a real client upload
  // (Alternatively you could just pass the path if running locally, but this is more robust testing)
  final bytes = File(imagePath).readAsBytesSync();
  final base64Image = base64Encode(bytes);
  final mimeType = _getMimeType(imagePath);
  final dataUri = 'data:$mimeType;base64,$base64Image';

  print('\n[Request] User: "Describe this image."');

  final messages = [
    {
      'role': 'user',
      'content': [
        {'type': 'text', 'text': 'Describe this image.'},
        {
          'type': 'image_url',
          'image_url': {'url': dataUri}
          // You can also use local path if you trust the server to read it:
          // 'image_url': {'url': imagePath}
        }
      ]
    }
  ];

  await _sendChat(client, sessionId, messages);
}

String _getMimeType(String path) {
  if (path.toLowerCase().endsWith('.png')) return 'image/png';
  if (path.toLowerCase().endsWith('.jpg') ||
      path.toLowerCase().endsWith('.jpeg')) return 'image/jpeg';
  if (path.toLowerCase().endsWith('.webp')) return 'image/webp';
  return 'application/octet-stream';
}

Future<void> _sendChat(HttpClient client, String sessionId,
    List<Map<String, dynamic>> messages) async {
  try {
    final request =
        await client.post('localhost', 8080, '/v1/chat/completions');
    request.headers.contentType = ContentType.json;
    request.headers.add('X-Session-ID', sessionId);

    final payload = {
      'model': 'vision-model',
      'messages': messages,
    };

    request.write(jsonEncode(payload));
    final response = await request.close();

    await response.transform(utf8.decoder).listen((data) {
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
    print('');
  } catch (e) {
    print('Error: $e');
  }
}
