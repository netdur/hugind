import 'dart:async';
import 'dart:collection';
import '../config/server_config.dart';
import 'llama_engine.dart';

class EngineManager {
  // Singleton
  static final EngineManager instance = EngineManager._();
  EngineManager._();

  final List<LlamaEngine> _engines = [];
  AsyncSemaphore? _generationSemaphore;
  int _maxQueueSize = 100;

  /// Returns a list of unique model names currently deployed
  List<String> get loadedModels {
    return _engines.map((e) => e.config.name).toSet().toList();
  }

  int get waitingCount => _generationSemaphore?.waitingCount ?? 0;
  int get activeCount => _generationSemaphore?.activeCount ?? 0;
  int get maxQueueSize => _maxQueueSize;
  bool get isQueueFull => waitingCount >= _maxQueueSize;

  void setMaxQueueSize(int value) {
    if (value < 0) {
      throw ArgumentError('maxQueueSize must be >= 0');
    }
    _maxQueueSize = value;
    _generationSemaphore?.maxQueueSize = value;
  }

  /// Deploy engines based on configuration
  Future<void> deploy(ServerConfig config) async {
    print('   → Deploying engine instance for model: ${config.modelPath}...');

    _generationSemaphore ??= AsyncSemaphore(
      config.maxSlots,
      maxQueueSize: _maxQueueSize,
    );

    // Single Engine Architecture (Batching)
    // We create ONE engine, but it will handle 'config.concurrency' via nSeqMax internally.
    final engine = LlamaEngine(config);
    await engine.init();
    _engines.add(engine);
    print('     ✓ Engine ready (Concurrency: ${config.concurrency})');
  }

  /// Route a request to the appropriate engine
  LlamaEngine getEngineForUser(String userId) {
    if (_engines.isEmpty) throw StateError("No engines deployed");
    // Single Engine: Always return the first/only one.
    return _engines.first;
  }

  Future<SemaphorePermit> acquireGenerationSlot(
      {Future<void>? cancelSignal}) async {
    final semaphore = _generationSemaphore;
    if (semaphore == null) {
      throw StateError('Generation semaphore not initialized');
    }
    return semaphore.acquire(cancelSignal: cancelSignal);
  }

  /// Force hibernate a user session across all engines
  Future<bool> hibernateSession(String userId) async {
    if (_engines.isEmpty) return false;
    return await _engines.first.hibernateSession(userId);
  }

  Future<void> dispose() async {
    print('   → Shutting down engine...');
    for (final e in _engines) {
      await e.dispose();
    }
    _engines.clear();
  }
}

class QueueFullException implements Exception {
  final String message = 'Queue is full';
  @override
  String toString() => 'QueueFullException: $message';
}

class QueueCancelledException implements Exception {
  final String message = 'Request cancelled while waiting in queue';
  @override
  String toString() => 'QueueCancelledException: $message';
}

class SemaphorePermit {
  final AsyncSemaphore _semaphore;
  bool _released = false;

  SemaphorePermit._(this._semaphore);

  void release() {
    if (_released) return;
    _released = true;
    _semaphore._release();
  }
}

class AsyncSemaphore {
  final int _maxPermits;
  int _available;
  int maxQueueSize;
  final Queue<_Waiter> _waiters = Queue<_Waiter>();

  AsyncSemaphore(
    int maxPermits, {
    required this.maxQueueSize,
  })  : _maxPermits = maxPermits,
        _available = maxPermits;

  int get waitingCount => _waiters.length;
  int get activeCount => _maxPermits - _available;

  Future<SemaphorePermit> acquire({Future<void>? cancelSignal}) async {
    if (_available > 0) {
      _available--;
      return SemaphorePermit._(this);
    }

    if (_waiters.length >= maxQueueSize) {
      throw QueueFullException();
    }

    final waiter = _Waiter();
    _waiters.add(waiter);

    if (cancelSignal != null) {
      cancelSignal.then((_) {
        if (waiter.completer.isCompleted) return;
        waiter.cancelled = true;
        _waiters.remove(waiter);
        waiter.completer.completeError(QueueCancelledException());
      });
    }

    await waiter.completer.future;
    return SemaphorePermit._(this);
  }

  void _release() {
    while (_waiters.isNotEmpty) {
      final next = _waiters.removeFirst();
      if (next.cancelled || next.completer.isCompleted) continue;
      try {
        next.completer.complete();
        return;
      } catch (_) {
        // If the waiter was completed concurrently, try the next.
      }
    }

    if (_available < _maxPermits) {
      _available++;
    }
  }
}

class _Waiter {
  final Completer<void> completer = Completer<void>();
  bool cancelled = false;
}
