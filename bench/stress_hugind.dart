import 'dart:convert';
import 'dart:io';

import 'package:args/args.dart';
import 'package:http/http.dart' as http;

class BenchResult {
  BenchResult({
    required this.requestId,
    required this.ttft,
    required this.totalTime,
    required this.tokenCount,
    required this.tpot,
  });

  final int requestId;
  final double? ttft;
  final double totalTime;
  final int tokenCount;
  final double tpot;
}

Future<BenchResult?> benchmarkRequest({
  required http.Client client,
  required String baseUrl,
  required String model,
  required String prompt,
  required int requestId,
  required Map<String, int> stats,
}) async {
  final baseUri = Uri.parse(baseUrl);
  final path = baseUri.path.replaceAll(RegExp(r'/+$'), '') + '/chat/completions';
  final requestUri = baseUri.replace(path: path);

  final payload = <String, dynamic>{
    'model': model,
    'messages': [
      {'role': 'user', 'content': prompt}
    ],
    'stream': true,
    'max_tokens': 100,
  };

  final request = http.Request('POST', requestUri);
  request.headers.addAll({
    'Content-Type': 'application/json',
    'Authorization': 'Bearer nopass',
  });
  request.body = jsonEncode(payload);

  final stopwatch = Stopwatch()..start();
  double? ttft;
  var tokenCount = 0;

  try {
    final response = await client
        .send(request)
        .timeout(const Duration(seconds: 300));
    if (response.statusCode != 200) {
      await response.stream.drain();
      if (response.statusCode == 429) {
        stats['queued'] = (stats['queued'] ?? 0) + 1;
      } else if (response.statusCode == 503) {
        stats['rejected'] = (stats['rejected'] ?? 0) + 1;
      } else {
        stats['errors'] = (stats['errors'] ?? 0) + 1;
      }
      throw HttpException('HTTP ${response.statusCode}');
    }

    await for (final line in response.stream
        .transform(utf8.decoder)
        .transform(const LineSplitter())) {
      final trimmed = line.trim();
      if (trimmed.isEmpty || !trimmed.startsWith('data: ')) {
        continue;
      }

      final dataStr = trimmed.substring(6);
      if (dataStr == '[DONE]') {
        break;
      }

      try {
        final data = jsonDecode(dataStr) as Map<String, dynamic>;
        ttft ??= stopwatch.elapsedMicroseconds / 1e6;
        final choices = data['choices'];
        if (choices is List && choices.isNotEmpty) {
          final delta = choices.first['delta'];
          if (delta is Map<String, dynamic> && delta['content'] != null) {
            tokenCount += 1;
          }
        }
      } on FormatException {
        continue;
      }
    }
  } catch (e) {
    stdout.writeln('req $requestId failed: $e');
    return null;
  } finally {
    stopwatch.stop();
  }

  final totalTime = stopwatch.elapsedMicroseconds / 1e6;
  final tpot = (ttft != null && tokenCount > 1)
      ? (totalTime - ttft) / (tokenCount - 1)
      : 0.0;

  stats['success'] = (stats['success'] ?? 0) + 1;
  stdout.writeln('req $requestId finished in ${totalTime.toStringAsFixed(2)} seconds');

  return BenchResult(
    requestId: requestId,
    ttft: ttft,
    totalTime: totalTime,
    tokenCount: tokenCount,
    tpot: tpot,
  );
}

double mean(List<double> values) =>
    values.isEmpty ? 0.0 : values.reduce((a, b) => a + b) / values.length;

Future<void> main(List<String> args) async {
  final parser = ArgParser()
    ..addOption('base-url', defaultsTo: 'http://localhost:8080/v1')
    ..addOption('model', defaultsTo: 'gpt-3.5-turbo')
    ..addOption('total', defaultsTo: '100', help: 'Total requests to send')
    ..addOption('prompt',
        defaultsTo: 'Explain quantum physics in one sentence.');

  final parsed = parser.parse(args);
  final baseUrl = parsed['base-url'] as String;
  final model = parsed['model'] as String;
  final total = int.tryParse(parsed['total'] as String) ?? 100;
  final prompt = parsed['prompt'] as String;

  stdout.writeln('Sending $total total requests');
  stdout.writeln('-' * 50);

  final stats = {'success': 0, 'queued': 0, 'rejected': 0, 'errors': 0};
  final client = http.Client();
  List<BenchResult> results;
  try {
    final futures = List.generate(
      total,
      (i) => benchmarkRequest(
        client: client,
        baseUrl: baseUrl,
        model: model,
        prompt: prompt,
        requestId: i,
        stats: stats,
      ),
    );
    results = (await Future.wait(futures)).whereType<BenchResult>().toList();
  } finally {
    client.close();
  }

  if (results.isEmpty) {
    return;
  }

  final totalTimes = results.map((r) => r.totalTime).toList();
  stdout.writeln('-' * 50);
  stdout.writeln('Completed ${results.length}/$total requests successfully.');
  stdout.writeln('Queued (429): ${stats['queued']}');
  stdout.writeln('Rejected (503): ${stats['rejected']}');
  stdout.writeln('Other errors: ${stats['errors']}');
  stdout.writeln(
      'Average response time: ${mean(totalTimes).toStringAsFixed(2)}s');
}
