import 'dart:convert';
import 'dart:io';

import 'package:http/http.dart' as http;
import 'package:test/test.dart';

/// Example for listing models via the OpenAI-compatible endpoint.
/// Run with a live server:
///   HUGIND_URL=http://127.0.0.1:8080 dart test test/api/models_test.dart
void main() {
  final baseUrl =
      Platform.environment['HUGIND_URL'] ?? 'http://127.0.0.1:8080';

  test(
    'GET /v1/models returns model list',
    () async {
      final response = await http.get(Uri.parse('$baseUrl/v1/models'));
      expect(response.statusCode, 200);
      expect(response.headers['content-type'], contains('application/json'));

      final body = jsonDecode(response.body) as Map<String, dynamic>;
      expect(body['object'], 'list');
      expect(body['data'], isA<List>());
    },
    skip:
        'Integration-style example; requires a running Hugind server at HUGIND_URL.',
  );
}
