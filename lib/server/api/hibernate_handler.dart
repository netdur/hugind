import 'dart:async';
import 'dart:convert';
import 'package:shelf/shelf.dart';
import '../engine/engine_manager.dart';

class HibernateHandler {
  Future<Response> call(Request request) async {
    try {
      String userId;

      // Try header first
      userId = request.headers['x-session-id'] ??
          request.headers['X-Session-ID'] ??
          '';

      // If not in header, check body
      if (userId.isEmpty) {
        final bodyString = await request.readAsString();
        if (bodyString.isNotEmpty) {
          final json = jsonDecode(bodyString);
          userId = json['user_id'] ?? json['session_id'] ?? '';
        }
      }

      if (userId.isEmpty) {
        return Response(400,
            body:
                jsonEncode({'error': 'Missing session ID in header or body'}));
      }

      print('📩 Hibernation Request for User: $userId');

      final success = await EngineManager.instance.hibernateSession(userId);

      if (success) {
        return Response.ok(
            jsonEncode({'status': 'hibernated', 'user_id': userId}),
            headers: {'content-type': 'application/json'});
      } else {
        // If not active or in RAM, it might be already on disk or just invalid.
        // We will assume if it's not found in memory, it's effectively "safe" or "not our problem".
        // But for clarity let's say "not_found_in_memory".
        return Response.ok(
            jsonEncode({
              'status': 'not_active',
              'user_id': userId,
              'message':
                  'Session was not in memory (already on disk or non-existent)'
            }),
            headers: {'content-type': 'application/json'});
      }
    } catch (e) {
      print('Hibernate Error: $e');
      return Response.internalServerError(
          body: jsonEncode({'error': e.toString()}));
    }
  }
}
