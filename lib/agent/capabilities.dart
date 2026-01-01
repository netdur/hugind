import 'dart:io';
import 'dart:async';
import 'package:interact/interact.dart';
import 'package:http/http.dart' as http;
import 'dart:convert';
import 'package:path/path.dart' as p;

class SysCapability {
  final List<String> allowedPaths;

  SysCapability({this.allowedPaths = const []});

  Future<String> run(String executable, List<String> args,
      {String? workDir}) async {
    if (workDir != null) {
      if (!_isAllowed(workDir)) {
        throw Exception(
            'Access denied to path: $workDir. Allowed: $allowedPaths');
      }
    }

    try {
      final result =
          await Process.run(executable, args, workingDirectory: workDir);
      if (result.exitCode != 0) {
        throw Exception(
            'Command failed (exit ${result.exitCode}): ${result.stderr}');
      }
      return result.stdout.toString().trim();
    } catch (e) {
      throw Exception('Execution failed: $e');
    }
  }

  Future<bool> confirm(String message) async {
    return Confirm(prompt: message, defaultValue: true).interact();
  }

  void printMsg(String message) {
    print(message);
  }

  bool _isAllowed(String path) {
    // Normalize and check against permitted paths
    final absPath = p.normalize(p.absolute(path));
    // Also resolve symlinks if possible, but for now simple string check

    for (var allowed in allowedPaths) {
      // expand allowed path if it's relative
      final absAllowed = p.normalize(p.absolute(allowed));
      if (absPath.startsWith(absAllowed)) return true;
    }
    return false;
  }
}

class LlmCapability {
  final String baseUrl;

  LlmCapability(this.baseUrl);

  Future<String> chat(String prompt, {String? system}) async {
    final uri = Uri.parse('$baseUrl/v1/chat/completions');

    final messages = <Map<String, String>>[];
    if (system != null && system.isNotEmpty) {
      messages.add({'role': 'system', 'content': system});
    }
    messages.add({'role': 'user', 'content': prompt});

    final body = jsonEncode({
      'messages': messages,
      'temperature': 0.7,
      'stream': false,
    });

    try {
      final resp = await http
          .post(uri, body: body, headers: {'Content-Type': 'application/json'});
      if (resp.statusCode != 200) {
        throw Exception('LLM error ${resp.statusCode}: ${resp.body}');
      }

      try {
        final data = jsonDecode(resp.body);
        final content = data['choices']?[0]?['message']?['content'];
        if (content == null) return '';
        return content.toString();
      } catch (e) {
        // Fallback: Try parsing as SSE string
        final lines = resp.body.split('\n');
        final buffer = StringBuffer();
        for (var line in lines) {
          if (line.startsWith('data: ')) {
            final jsonStr = line.substring(6).trim();
            if (jsonStr == '[DONE]') continue;
            try {
              final chunk = jsonDecode(jsonStr);
              // Check for delta content
              final delta = chunk['choices']?[0]?['delta']?['content'];
              if (delta != null) {
                buffer.write(delta);
                continue;
              }
              // Check for message content (if non-streaming format sent in SSE wrapper)
              final content = chunk['choices']?[0]?['message']?['content'];
              if (content != null) {
                buffer.write(content);
              }
            } catch (_) {
              // ignore chunk parse error
            }
          }
        }
        final result = buffer.toString();
        if (result.isEmpty)
          throw e; // Rethrow original json error if no SSE content found
        return result;
      }
    } catch (e) {
      throw Exception('LLM request failed: $e');
    }
  }
}
