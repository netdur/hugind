import 'dart:io';

import 'package:http/http.dart' as http;
import 'package:test/test.dart';

/// Simple health check example/integration test.
/// Run the Hugind server locally, then execute:
///   HUGIND_URL=http://127.0.0.1:8080 dart test test/api/health_test.dart
void main() {
  final baseUrl =
      Platform.environment['HUGIND_URL'] ?? 'http://127.0.0.1:8080';

  test(
    'GET /health returns ok',
    () async {
      final response = await http.get(Uri.parse('$baseUrl/health'));
      expect(response.statusCode, 200);
      expect(response.headers['content-type'], contains('application/json'));
      expect(response.body, contains('"status":"ok"'));
    },
    skip:
        'Integration-style example; requires a running Hugind server at HUGIND_URL.',
  );
}
