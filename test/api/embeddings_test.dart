import 'dart:convert';
import 'dart:io';

import 'package:http/http.dart' as http;
import 'package:test/test.dart';

/// Example for the /v1/embeddings endpoint (requires embeddings-enabled server).
/// Run with a live embeddings server:
///   HUGIND_URL=http://127.0.0.1:8080 dart test test/api/embeddings_test.dart
void main() {
  final baseUrl =
      Platform.environment['HUGIND_URL'] ?? 'http://127.0.0.1:8080';

  test(
    'POST /v1/embeddings returns vectors',
    () async {
      final response = await http.post(
        Uri.parse('$baseUrl/v1/embeddings'),
        headers: {'content-type': 'application/json'},
        body: jsonEncode({
          'model': 'default',
          'input': ['hello world', 'another input'],
          'user': 'embeddings_example',
        }),
      );

      expect(response.statusCode, 200);
      expect(response.headers['content-type'], contains('application/json'));

      final body = jsonDecode(response.body) as Map<String, dynamic>;
      expect(body['object'], 'list');
      expect(body['data'], isA<List>());
      expect(body['data'][0]['embedding'], isA<List>());
    },
    skip:
        'Integration-style example; requires a running embeddings-enabled Hugind server at HUGIND_URL.',
  );
}
