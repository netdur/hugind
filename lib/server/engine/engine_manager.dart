import 'dart:async';
import '../config/server_config.dart';
import 'llama_engine.dart';

class EngineManager {
  // Singleton
  static final EngineManager instance = EngineManager._();
  EngineManager._();

  final List<LlamaEngine> _engines = [];
  int _rrIndex = 0; // Round-robin index

  /// Returns a list of unique model names currently deployed
  List<String> get loadedModels {
    return _engines.map((e) => e.config.name).toSet().toList();
  }

  /// Deploy engines based on configuration
  Future<void> deploy(ServerConfig config) async {
    print('   → Deploying ${config.concurrency} engine instance(s)...');

    for (int i = 0; i < config.concurrency; i++) {
      final engine = LlamaEngine(config);
      await engine.init();
      _engines.add(engine);
      print('     ✓ Instance #${i + 1} ready');
    }
  }

  /// Route a request to the appropriate engine
  LlamaEngine getEngineForUser(String userId) {
    if (_engines.isEmpty) throw StateError("No engines deployed");

    // 1. Session Affinity Check
    // If a user already has a session on an engine, send them back there.
    // (Currently LlamaEngine._activeSessions is private, in a real app
    // we would expose a method 'hasUser(id)' or track it here).
    // For now, we will trust the simple Load Balancing.

    // 2. Simple Round Robin (for now)
    // In a multi-slot setup, this distributes NEW users across engines.
    final engine = _engines[_rrIndex];
    _rrIndex = (_rrIndex + 1) % _engines.length;

    return engine;
  }

  Future<void> dispose() async {
    print('   → Shutting down engines...');
    for (final e in _engines) {
      await e.dispose();
    }
    _engines.clear();
  }
}
