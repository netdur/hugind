import 'dart:convert';
import 'dart:io';

import 'package:http/http.dart' as http;
import 'package:test/test.dart';

/// Example for multimodal chat (image + text). Requires a vision-enabled model
/// and mmproj. Run with a live server:
///   HUGIND_URL=http://127.0.0.1:8080 dart test test/api/chat_vision_test.dart --run-skipped
void main() {
  final baseUrl =
      Platform.environment['HUGIND_URL'] ?? 'http://127.0.0.1:8080';

  // 1x1 PNG (red pixel) data URL.
  const dataUrl =
      'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y0nUgAAAABJRU5ErkJggg==';

  test(
    'POST /v1/chat/completions with image streams SSE',
    () async {
      final request = http.Request(
        'POST',
        Uri.parse('$baseUrl/v1/chat/completions'),
      )..headers['content-type'] = 'application/json'
        ..body = jsonEncode({
          'model': 'default',
          'messages': [
            {
              'role': 'user',
              'content': [
                {'type': 'text', 'text': 'Describe this image succinctly.'},
                {'type': 'image_url', 'image_url': {'url': dataUrl}},
              ],
            }
          ],
          'user': 'chat_vision_example',
        });

      final streamed = await request.send();
      expect(streamed.statusCode, 200);
      expect(streamed.headers['content-type'], contains('text/event-stream'));

      final firstChunk = await streamed.stream
          .transform(utf8.decoder)
          .transform(const LineSplitter())
          .firstWhere((line) => line.startsWith('data:'));

      // Log the first SSE line so you can see the model respond during the test.
      // This should include the JSON chunk with streamed text.
      // Example: data: {"id":"...","object":"chat.completion.chunk",...}
      // You can run with `--reporter expanded` for even more detail.
      // ignore: avoid_print
      print('First SSE line: $firstChunk');

      expect(firstChunk, startsWith('data: '));
    },
    skip:
        'Integration-style example; requires a running vision-enabled Hugind server with mmproj.',
  );
}
