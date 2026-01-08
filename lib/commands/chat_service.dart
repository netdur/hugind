import 'dart:convert';
import 'package:http/http.dart' as http;

class ChatService {
  final String baseUrl = 'http://localhost:8080/v1/chat';

  /// Sends a message. If server returns 409, it automatically resends full history.
  Future<http.StreamedResponse> sendMessage({
    required String sessionId,
    required String model,
    required List<dynamic> fullHistory,
    required Map<String, dynamic> newMessage,
  }) async {
    final uri = Uri.parse('$baseUrl/completions');
    final client = http.Client();

    // 1. Optimistic Attempt: Send only the new message
    var payload = {
      'model': model,
      'messages': [newMessage], // efficient
      'stream': true
    };

    var request = http.Request('POST', uri);
    request.headers.addAll({
      'Content-Type': 'application/json',
      'X-Session-ID': sessionId,
    });
    request.body = jsonEncode(payload);

    var response = await client.send(request);

    // 2. Fallback Attempt: If Context Lost (409), send everything
    if (response.statusCode == 409) {
      print('   ⚠️  Server cache cold/missing. Re-hydrating context...');

      // Update payload to include full history
      // Note: We add the new message to history temporarily for the request
      final rehydratePayload = {
        'model': model,
        'messages': [...fullHistory, newMessage],
        'stream': true
      };

      final retryReq = http.Request('POST', uri);
      retryReq.headers.addAll({
        'Content-Type': 'application/json',
        'X-Session-ID': sessionId,
        'X-Fresh-Session': 'true', // Tell server to overwrite KV cache
      });
      retryReq.body = jsonEncode(rehydratePayload);

      response = await client.send(retryReq);
    }

    return response;
  }

  Future<void> hibernate(String id) async {
    try {
      await http
          .post(Uri.parse('$baseUrl/hibernate'), headers: {'X-Session-ID': id});
    } catch (_) {}
  }
}
