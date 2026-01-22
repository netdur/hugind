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
    'X-Fresh-Session': 'false',
  });
  request.body = jsonEncode(payload);

  final stopwatch = Stopwatch()..start();
  double? ttft;
  var tokenCount = 0;

  try {
    final response = await client.send(request);
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
    stderr.writeln('Request $requestId failed: $e');
    return null;
  } finally {
    stopwatch.stop();
  }

  final totalTime = stopwatch.elapsedMicroseconds / 1e6;
  final tpot = (ttft != null && tokenCount > 1)
      ? (totalTime - ttft) / (tokenCount - 1)
      : 0.0;

  return BenchResult(
    requestId: requestId,
    ttft: ttft,
    totalTime: totalTime,
    tokenCount: tokenCount,
    tpot: tpot,
  );
}

String formatFixed(double value) => value.toStringAsFixed(4);

double mean(List<double> values) =>
    values.isEmpty ? 0.0 : values.reduce((a, b) => a + b) / values.length;

Future<void> main(List<String> args) async {
  final parser = ArgParser()
    ..addOption('base-url',
        defaultsTo: 'http://localhost:8080/v1',
        help: 'Hugind API URL')
    ..addOption('model',
        defaultsTo: 'gpt-3.5-turbo', help: 'Model name')
    ..addOption('concurrency',
        defaultsTo: '10', help: 'Number of concurrent requests')
    ..addOption('prompt',
        defaultsTo: 'Explain quantum physics in one sentence.',
        help: 'Prompt to send');

  final parsed = parser.parse(args);
  final baseUrl = parsed['base-url'] as String;
  final model = parsed['model'] as String;
  final concurrency = int.tryParse(parsed['concurrency'] as String) ?? 10;
  final prompt = parsed['prompt'] as String;

  stdout.writeln('Benchmarking Hugind at $baseUrl');
  stdout.writeln('Model: $model');
  stdout.writeln('Concurrency: $concurrency');
  stdout.writeln('Prompt: $prompt');
  stdout.writeln('-' * 50);

  final client = http.Client();
  List<BenchResult> results;
  try {
    final futures = List.generate(
      concurrency,
      (i) => benchmarkRequest(
        client: client,
        baseUrl: baseUrl,
        model: model,
        prompt: prompt,
        requestId: i,
      ),
    );
    results = (await Future.wait(futures)).whereType<BenchResult>().toList();
  } finally {
    client.close();
  }

  if (results.isEmpty) {
    stdout.writeln('No successful requests.');
    return;
  }

  final ttfts = results
      .where((r) => r.ttft != null)
      .map((r) => r.ttft!)
      .toList();
  final totalTimes = results.map((r) => r.totalTime).toList();
  final tpots = results.map((r) => r.tpot).toList();

  if (ttfts.isEmpty) {
    stdout.writeln('No tokens received.');
    return;
  }

  stdout.writeln('\nResults:');
  stdout.writeln('Successful Requests: ${results.length}/$concurrency');
  stdout.writeln('Avg TTFT: ${formatFixed(mean(ttfts))}s');
  stdout.writeln('Avg Total Time: ${formatFixed(mean(totalTimes))}s');
  stdout.writeln('Avg TPOT: ${formatFixed(mean(tpots))}s');

  stdout.writeln('\nDetailed:');
  stdout.writeln('ID    TTFT (s)   Total (s)  Tokens   TPOT (s)');
  results.sort((a, b) => a.requestId.compareTo(b.requestId));
  for (final r in results) {
    final tTtft = r.ttft == null ? 'N/A' : formatFixed(r.ttft!);
    stdout.writeln(
        '${r.requestId.toString().padRight(5)}${tTtft.padRight(11)}${formatFixed(r.totalTime).padRight(11)}${r.tokenCount.toString().padRight(9)}${formatFixed(r.tpot)}');
  }
}
