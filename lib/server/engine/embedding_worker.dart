import 'dart:async';
import 'dart:isolate';

import 'package:llama_cpp_dart/llama_cpp_dart.dart';

import '../config/server_config.dart';

/// Lightweight isolate wrapper that keeps a dedicated Llama instance
/// alive for embeddings so the HTTP isolate stays responsive.
class EmbeddingsWorker {
  EmbeddingsWorker({
    required this.modelPath,
    required this.libraryPath,
    required this.modelParams,
    required this.contextParams,
  });

  factory EmbeddingsWorker.fromConfig(ServerConfig config) {
    return EmbeddingsWorker(
      modelPath: config.modelPath,
      libraryPath: config.libraryPath ?? Llama.libraryPath,
      modelParams: config.modelParams,
      contextParams: config.contextParams,
    );
  }

  final String modelPath;
  final String? libraryPath;
  final ModelParams modelParams;
  final ContextParams contextParams;

  Isolate? _isolate;
  SendPort? _sendPort;
  ReceivePort? _responsePort;
  ReceivePort? _errorPort;
  ReceivePort? _exitPort;
  StreamSubscription? _responseSub;
  StreamSubscription? _errorSub;
  StreamSubscription? _exitSub;
  Future<void>? _startFuture;
  int _requestId = 0;
  final Map<int, Completer<List<double>>> _pending = {};

  Future<void> start() {
    return _startFuture ??= _startInternal();
  }

  Future<List<double>> embed(String input) async {
    final prompt = input.trim();
    if (prompt.isEmpty) return const [];
    await start();
    final port = _sendPort;
    if (port == null) {
      throw StateError('Embedding isolate is not ready.');
    }

    final completer = Completer<List<double>>();
    final id = _requestId++;
    _pending[id] = completer;
    port.send({'type': 'embed', 'id': id, 'input': prompt});
    return completer.future;
  }

  Future<void> dispose() async {
    _sendPort?.send({'type': 'shutdown'});
    _isolate?.kill(priority: Isolate.immediate);

    await _responseSub?.cancel();
    await _errorSub?.cancel();
    await _exitSub?.cancel();

    _responsePort?.close();
    _errorPort?.close();
    _exitPort?.close();

    _isolate = null;
    _sendPort = null;
    _startFuture = null;

    final error = StateError('Embedding worker disposed');
    _failAll(error);
  }

  Future<void> _startInternal() async {
    final ready = Completer<void>();
    _responsePort = ReceivePort();
    _errorPort = ReceivePort();
    _exitPort = ReceivePort();

    _responseSub = _responsePort!.listen((message) {
      if (message is Map) {
        final type = message['type'];
        if (type == 'ready') {
          _sendPort = message['port'] as SendPort?;
          if (_sendPort != null && !ready.isCompleted) {
            ready.complete();
          }
        } else if (type == 'result') {
          final id = message['id'] as int;
          final embedding =
              (message['embedding'] as List).map((e) => e as double).toList();
          _pending.remove(id)?.complete(embedding);
        } else if (type == 'error') {
          final id = message['id'] as int?;
          final err = Exception(message['error']);
          if (id != null) {
            _pending.remove(id)?.completeError(err);
          } else {
            _failAll(err);
          }
        } else if (type == 'initError') {
          final err =
              Exception('Embedding isolate failed: ${message['error']}');
          if (!ready.isCompleted) ready.completeError(err);
          _failAll(err);
        }
      }
    });

    _errorSub = _errorPort!.listen((message) {
      final err = _parseIsolateError(message);
      if (!ready.isCompleted) ready.completeError(err);
      _failAll(err);
    });

    _exitSub = _exitPort!.listen((_) {
      final err = StateError('Embedding isolate exited unexpectedly');
      if (!ready.isCompleted) ready.completeError(err);
      _failAll(err);
    });

    final initPayload = {
      'port': _responsePort!.sendPort,
      'modelPath': modelPath,
      'libraryPath': libraryPath,
      'modelParams': _serializeModelParams(modelParams),
      'contextParams': _serializeContextParams(contextParams),
    };

    _isolate = await Isolate.spawn<Map<String, Object?>>(
      _embeddingIsolate,
      initPayload,
      onError: _errorPort!.sendPort,
      onExit: _exitPort!.sendPort,
    );

    await ready.future;
  }

  void _failAll(Object error) {
    for (final c in _pending.values) {
      if (!c.isCompleted) c.completeError(error);
    }
    _pending.clear();
  }

  Exception _parseIsolateError(dynamic message) {
    if (message is List && message.isNotEmpty) {
      return Exception(message.first.toString());
    }
    return Exception(message?.toString() ?? 'Unknown isolate error');
  }
}

void _embeddingIsolate(Map<String, Object?> payload) async {
  final SendPort replyPort = payload['port'] as SendPort;
  final modelPath = payload['modelPath'] as String;
  final libraryPath = payload['libraryPath'] as String?;
  final modelParams = (payload['modelParams'] as Map).cast<String, Object?>();
  final contextParams =
      (payload['contextParams'] as Map).cast<String, Object?>();

  final requestPort = ReceivePort();
  Llama? llama;
  try {
    if (libraryPath != null && libraryPath.isNotEmpty) {
      Llama.libraryPath = libraryPath;
    }

    final model = _buildModelParams(modelParams);
    final ctx = _buildContextParams(contextParams)..embeddings = true;

    llama = Llama(
      modelPath,
      modelParams: model,
      contextParams: ctx,
    );
  } catch (e, stack) {
    replyPort.send({
      'type': 'initError',
      'error': e.toString(),
      'stack': stack.toString(),
    });
    requestPort.close();
    return;
  }

  replyPort.send({'type': 'ready', 'port': requestPort.sendPort});

  await for (final message in requestPort) {
    if (message is Map && message['type'] == 'embed') {
      final id = message['id'] as int;
      final input = message['input'] as String;
      try {
        final embeddings = llama!.getEmbeddings(input);
        replyPort.send({'type': 'result', 'id': id, 'embedding': embeddings});
      } catch (e, stack) {
        replyPort.send({
          'type': 'error',
          'id': id,
          'error': '$e\n$stack',
        });
      }
    } else if (message is Map && message['type'] == 'shutdown') {
      break;
    }
  }

  llama?.dispose();
  requestPort.close();
  Isolate.exit();
}

Map<String, Object?> _serializeModelParams(ModelParams params) {
  return {
    'nGpuLayers': params.nGpuLayers,
    'splitMode': params.splitMode.name,
    'mainGpu': params.mainGpu,
    'useMemorymap': params.useMemorymap,
    'useMemoryLock': params.useMemoryLock,
    'vocabOnly': params.vocabOnly,
  };
}

Map<String, Object?> _serializeContextParams(ContextParams params) {
  return {
    'nCtx': params.nCtx,
    'nBatch': params.nBatch,
    'nUbatch': params.nUbatch,
    'nThreads': params.nThreads,
    'nThreadsBatch': params.nThreadsBatch,
    'flashAttention': params.flashAttention.name,
    'typeK': params.typeK.name,
    'typeV': params.typeV.name,
    'offloadKqv': params.offloadKqv,
    'embeddings': params.embeddings,
  };
}

ModelParams _buildModelParams(Map<String, Object?> raw) {
  final params = ModelParams();
  params.nGpuLayers = raw['nGpuLayers'] as int? ?? params.nGpuLayers;
  final split = raw['splitMode']?.toString();
  if (split != null && split.isNotEmpty) {
    params.splitMode = LlamaSplitMode.values
        .firstWhere((e) => e.name == split, orElse: () => params.splitMode);
  }
  params.mainGpu = raw['mainGpu'] as int? ?? params.mainGpu;
  params.useMemorymap = raw['useMemorymap'] as bool? ?? params.useMemorymap;
  params.useMemoryLock = raw['useMemoryLock'] as bool? ?? params.useMemoryLock;
  params.vocabOnly = raw['vocabOnly'] as bool? ?? params.vocabOnly;
  return params;
}

ContextParams _buildContextParams(Map<String, Object?> raw) {
  final params = ContextParams();
  params.nCtx = raw['nCtx'] as int? ?? params.nCtx;
  params.nBatch = raw['nBatch'] as int? ?? params.nBatch;
  params.nUbatch = raw['nUbatch'] as int? ?? params.nUbatch;
  params.nThreads = raw['nThreads'] as int? ?? params.nThreads;
  params.nThreadsBatch = raw['nThreadsBatch'] as int? ?? params.nThreadsBatch;

  final flash = raw['flashAttention']?.toString();
  if (flash != null && flash.isNotEmpty) {
    params.flashAttention = LlamaFlashAttnType.values.firstWhere(
      (e) => e.name == flash,
      orElse: () => params.flashAttention,
    );
  }

  final typeK = raw['typeK']?.toString();
  if (typeK != null && typeK.isNotEmpty) {
    params.typeK = LlamaKvCacheType.values
        .firstWhere((e) => e.name == typeK, orElse: () => params.typeK);
  }

  final typeV = raw['typeV']?.toString();
  if (typeV != null && typeV.isNotEmpty) {
    params.typeV = LlamaKvCacheType.values
        .firstWhere((e) => e.name == typeV, orElse: () => params.typeV);
  }

  params.offloadKqv = raw['offloadKqv'] as bool? ?? params.offloadKqv;
  params.embeddings = true;
  return params;
}
