import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'package:http/http.dart' as http;
import 'package:path/path.dart' as p;
import 'package:yaml/yaml.dart';

class ChatService {
  ChatService({String? defaultBaseUrl})
      : defaultBaseUrl = defaultBaseUrl ?? 'http://localhost:8080/v1/chat';

  final String defaultBaseUrl;

  Future<String> resolveBaseUrl(String configName) async {
    if (configName.isEmpty) return defaultBaseUrl;
    final configPath = p.join(_configHome(), 'configs', '$configName.yml');
    final file = File(configPath);
    if (!await file.exists()) return defaultBaseUrl;

    try {
      final content = await file.readAsString();
      final yaml = loadYaml(content);
      final server = yaml['server'] as Map? ?? {};
      final host = server['host']?.toString() ?? '127.0.0.1';
      final port = int.tryParse(server['port']?.toString() ?? '') ?? 8080;
      return 'http://$host:$port/v1/chat';
    } catch (_) {
      return defaultBaseUrl;
    }
  }

  /// Sends a message. If server returns 409, it automatically resends full history.
  Future<http.StreamedResponse> sendMessage({
    required String sessionId,
    required String model,
    required List<dynamic> fullHistory,
    required Map<String, dynamic> newMessage,
    bool isNewSession = false,
    String? baseUrl,
  }) async {
    final client = http.Client();
    final uri =
        Uri.parse('${baseUrl ?? defaultBaseUrl}/completions');

    try {
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
        'X-Fresh-Session': isNewSession.toString(),
      });
      request.body = jsonEncode(payload);

      var response = await client.send(request);

      // 2. Fallback Attempt: If Context Lost (409), send everything
      if (response.statusCode == 409) {
        print('   ⚠️  Server cache cold/missing. Re-hydrating context...');
        await response.stream.drain();

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
          'X-Fresh-Session': isNewSession.toString(),
        });
        retryReq.body = jsonEncode(rehydratePayload);

        response = await client.send(retryReq);
      }

      return _wrapResponse(response, client);
    } catch (e) {
      client.close();
      rethrow;
    }
  }

  Future<void> hibernate(String id, {String? baseUrl}) async {
    try {
      await http.post(Uri.parse('${baseUrl ?? defaultBaseUrl}/hibernate'),
          headers: {'X-Session-ID': id}).timeout(const Duration(seconds: 1));
    } catch (_) {}
  }

  http.StreamedResponse _wrapResponse(
      http.StreamedResponse response, http.Client client) {
    final stream = response.stream
        .transform(StreamTransformer<List<int>, List<int>>.fromHandlers(
      handleData: (data, sink) => sink.add(data),
      handleError: (error, stackTrace, sink) {
        client.close();
        sink.addError(error, stackTrace);
      },
      handleDone: (sink) {
        client.close();
        sink.close();
      },
    ));

    return http.StreamedResponse(
      stream,
      response.statusCode,
      contentLength: response.contentLength,
      request: response.request,
      headers: response.headers,
      isRedirect: response.isRedirect,
      persistentConnection: response.persistentConnection,
      reasonPhrase: response.reasonPhrase,
    );
  }
}

String _configHome() {
  final env = Platform.environment;
  if (Platform.isWindows) {
    final appData = env['APPDATA'];
    if (appData != null) return p.join(appData, 'hugind');
    return p.join(env['USERPROFILE'] ?? '.', '.hugind');
  }
  final xdg = env['XDG_CONFIG_HOME'];
  if (xdg != null && xdg.isNotEmpty) return p.join(xdg, 'hugind');
  return p.join(env['HOME'] ?? '.', '.hugind');
}
