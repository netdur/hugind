import 'dart:convert';
import 'dart:io';

import 'package:http/http.dart' as http;
import 'package:test/test.dart';

/// Example for the streaming chat completions endpoint.
/// Run with a live server (generation must be enabled, not embeddings-only):
///   HUGIND_URL=http://127.0.0.1:8080 dart test test/api/chat_completions_test.dart
void main() {
  final baseUrl =
      Platform.environment['HUGIND_URL'] ?? 'http://127.0.0.1:8080';

  test(
    'POST /v1/chat/completions streams SSE',
    () async {
      final request = http.Request(
        'POST',
        Uri.parse('$baseUrl/v1/chat/completions'),
      )..headers['content-type'] = 'application/json'
        ..body = jsonEncode({
          'model': 'default',
          'messages': [
            {'role': 'user', 'content': 'Hello! Respond with one word.'}
          ],
          'user': 'chat_test_user',
        });

      final streamed = await request.send();
      expect(streamed.statusCode, 200);
      expect(
        streamed.headers['content-type'],
        contains('text/event-stream'),
      );

      // Read a small portion of the stream as a sanity check/example.
      final firstChunk = await streamed.stream
          .transform(utf8.decoder)
          .transform(const LineSplitter())
          .firstWhere((line) => line.startsWith('data:'));

      expect(firstChunk, startsWith('data: '));
    },
    skip:
        'Integration-style example; requires a running Hugind server at HUGIND_URL.',
  );
}
