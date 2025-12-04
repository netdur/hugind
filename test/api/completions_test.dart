import 'dart:convert';
import 'dart:io';

import 'package:http/http.dart' as http;
import 'package:test/test.dart';

/// Examples for the legacy /v1/completions endpoint (non-stream and stream).
/// Run with a live server (generation enabled):
///   HUGIND_URL=http://127.0.0.1:8080 dart test test/api/completions_test.dart
void main() {
  final baseUrl =
      Platform.environment['HUGIND_URL'] ?? 'http://127.0.0.1:8080';

  test(
    'POST /v1/completions (non-stream)',
    () async {
      final response = await http.post(
        Uri.parse('$baseUrl/v1/completions'),
        headers: {'content-type': 'application/json'},
        body: jsonEncode({
          'model': 'default',
          'prompt': 'Say hello in two words.',
          'user': 'completions_example',
        }),
      );

      expect(response.statusCode, 200);
      expect(response.headers['content-type'], contains('application/json'));

      final body = jsonDecode(response.body) as Map<String, dynamic>;
      expect(body['object'], 'text_completion');
      expect(body['choices'], isA<List>());
      expect(body['choices'][0]['text'], isA<String>());
    },
    skip:
        'Integration-style example; requires a running Hugind server at HUGIND_URL.',
  );

  test(
    'POST /v1/completions (stream)',
    () async {
      final request = http.Request(
        'POST',
        Uri.parse('$baseUrl/v1/completions'),
      )..headers['content-type'] = 'application/json'
        ..body = jsonEncode({
          'model': 'default',
          'prompt': 'List one color.',
          'stream': true,
          'user': 'completions_stream_example',
        });

      final streamed = await request.send();
      expect(streamed.statusCode, 200);
      expect(
        streamed.headers['content-type'],
        contains('text/event-stream'),
      );

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
