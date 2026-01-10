import 'dart:async';
import '../config/server_config.dart';
import 'llama_engine.dart';

class EngineManager {
  // Singleton
  static final EngineManager instance = EngineManager._();
  EngineManager._();

  final List<LlamaEngine> _engines = [];

  /// Returns a list of unique model names currently deployed
  List<String> get loadedModels {
    return _engines.map((e) => e.config.name).toSet().toList();
  }

  /// Deploy engines based on configuration
  Future<void> deploy(ServerConfig config) async {
    print('   → Deploying engine instance for model: ${config.modelPath}...');

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
