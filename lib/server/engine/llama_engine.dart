import 'dart:async';
import 'dart:io';
import 'dart:typed_data';
import 'package:path/path.dart' as p;
import 'package:llama_cpp_dart/llama_cpp_dart.dart';
import '../config/server_config.dart';
import 'embedding_worker.dart';
import '../../global_settings.dart';

class LlamaEngine {
  final ServerConfig config;
  final bool _embeddingsOnly;
  final LlamaParent? _parent;
  final EmbeddingsWorker? _embeddingsWorker;

  // --- TIER 1: VRAM (Hot) ---
  final Map<String, LlamaScope> _activeSessions = {};

  // --- TIER 2: System RAM (Warm) ---
  // Stores the serialized state to avoid re-tokenization
  final Map<String, Uint8List> _ramSessions = {};

  // --- METADATA ---
  final Map<String, DateTime> _lastUsed = {};

  // Maintenance Timer for Tier 2 -> Tier 3 migration
  Timer? _maintenanceTimer;
  final Duration _ramTtl = Duration(minutes: 60);

  late Directory _sessionsDir;

  LlamaEngine(this.config)
      : _embeddingsOnly = config.embeddingsEnabled,
        _parent = config.embeddingsEnabled
            ? null
            : LlamaParent(LlamaLoad(
                path: config.modelPath,
                modelParams: config.modelParams,
                contextParams: config.contextParams,
                samplingParams: config.samplerParams,
                mmprojPath: config.mmprojPath,
              )),
        _embeddingsWorker = config.embeddingsEnabled
            ? EmbeddingsWorker.fromConfig(config)
            : null;

  Future<void> init() async {
    if (_embeddingsOnly) {
      await _embeddingsWorker?.start();
      return;
    }

    await _parent!.init();

    // Embedding-only configs do not use session caching.
    if (config.embeddingsEnabled) {
      return;
    }

    // Ensure session directory exists
    final sessionsPath = await GlobalSettings.getSessionsPath();
    _sessionsDir = Directory(sessionsPath);
    if (!await _sessionsDir.exists()) {
      await _sessionsDir.create(recursive: true);
    }

    // Start background maintenance loop (Every 1 minute)
    _maintenanceTimer = Timer.periodic(Duration(minutes: 1), (timer) {
      _runMaintenance();
    });
  }

  Future<List<double>> embed(String input) async {
    if (!config.embeddingsEnabled) {
      throw StateError('Embeddings are not enabled for this server.');
    }
    final prompt = input.trim();
    if (prompt.isEmpty) return const [];
    if (_embeddingsOnly) {
      final worker = _embeddingsWorker;
      if (worker == null) {
        throw StateError('Embedding worker not initialized');
      }
      return worker.embed(prompt);
    }
    return _parent!.getEmbeddings(prompt);
  }

  Stream<String> generateStream(String userId, List<Message> messages,
      {bool isFreshSession = true}) {
    if (_embeddingsOnly) {
      throw StateError('Text generation is disabled in embeddings-only mode.');
    }

    late StreamController<String> controller;
    final scopeCompleter = Completer<LlamaScope>();

    controller = StreamController<String>(
      onCancel: () {
        if (scopeCompleter.isCompleted) {
          scopeCompleter.future.then((scope) {
            print('   🛑 Client disconnected ($userId). Stopping generation.');
            scope.stop();
          });
        }
      },
    );

    _executeGeneration(
        userId, messages, controller, scopeCompleter, isFreshSession);
    return controller.stream;
  }

  Future<void> _executeGeneration(
    String userId,
    List<Message> messages,
    StreamController<String> controller,
    Completer<LlamaScope> scopeCompleter,
    bool isFreshSession,
  ) async {
    if (_parent == null) {
      controller.addError(
          StateError('Text generation is disabled in embeddings-only mode.'));
      await controller.close();
      return;
    }
    try {
      LlamaScope scope;
      bool isNewSession = false;

      // ---------------------------------------------------------
      // 0. HANDLE FORCED RESET
      // ---------------------------------------------------------
      if (isFreshSession) {
        if (_activeSessions.containsKey(userId)) {
          print('   🔄 [Reset] Clearing active session for $userId');
          await _activeSessions[userId]!.dispose();
          _activeSessions.remove(userId);
        }
        _ramSessions.remove(userId);
        // Note: We don't delete the disk file here, but we ignore it below.
      }

      // ---------------------------------------------------------
      // 1. RESOLVE SESSION (Tier 1 -> Tier 2 -> Tier 3 -> New)
      // ---------------------------------------------------------

      if (!isFreshSession && _activeSessions.containsKey(userId)) {
        // [TIER 1] HOT HIT: User is already in VRAM
        scope = _activeSessions[userId]!;
        print('   🔥 [Tier 1] VRAM Hit: $userId (Slot: ${scope.id})');
      } else if (!isFreshSession && _ramSessions.containsKey(userId)) {
        // [TIER 2] WARM HIT: User is in RAM, needs a VRAM slot
        print('   🧊 [Tier 2] RAM Hit: $userId. Restoring...');
        scope = await _allocateSlotWithEviction(userId);

        final stateData = _ramSessions[userId]!;
        await scope.loadState(stateData); // Restore memory
        _ramSessions.remove(userId);
      } else if (!isFreshSession && _diskSessionExists(userId)) {
        // [TIER 3] COLD HIT: User is on Disk, needs a VRAM slot
        final path = _getDiskPath(userId);
        print('   💾 [Tier 3] Disk Hit: $userId. Loading from file: $path');
        scope = await _allocateSlotWithEviction(userId);

        await scope.loadSession(_getDiskPath(userId));
      } else {
        // [NEW] No history found (or Forced Fresh)
        if (!isFreshSession) {
          throw ContextLostException();
        }

        print('   ✨ [New] Creating fresh session: $userId');
        scope = await _allocateSlotWithEviction(userId);
        isNewSession = true;
      }

      // Update timestamp for LRU Logic
      _lastUsed[userId] = DateTime.now();
      if (!scopeCompleter.isCompleted) scopeCompleter.complete(scope);

      // ---------------------------------------------------------
      // 2. PREPARE PROMPT
      // ---------------------------------------------------------
      final format = config.chatFormat ?? ChatFormat.chatml;

      final history = ChatHistory();
      for (var m in messages)
        history.addMessage(role: m.role, content: m.content, images: m.images);

      if (isNewSession) {
        if (history.messages.isEmpty ||
            history.messages.first.role != Role.system) {
          history.messages.insert(
              0, Message(role: Role.system, content: config.systemPrompt));
        }
      }

      final hasImages = history.images.isNotEmpty;
      String prompt;
      List<LlamaInput>? mediaInputs;

      if (hasImages) {
        final exported =
            history.exportWithMedia(format, leaveLastAssistantOpen: true);
        prompt = exported.$1;
        mediaInputs = exported.$2;
      } else if (isNewSession) {
        prompt = history.exportFormat(format, leaveLastAssistantOpen: true);
      } else {
        // Active OR Restored session: Just append the latest turn
        prompt = history.getLatestTurn(format);
      }

      if (prompt.trim().isEmpty) {
        await controller.close();
        return;
      }

      // ---------------------------------------------------------
      // 3. EXECUTE
      // ---------------------------------------------------------
      print('   🧠 Processing prompt for $userId (${prompt.length} chars)...');

      String promptId;
      if (hasImages) {
        final inputs = (mediaInputs ?? []).whereType<LlamaImage>().toList();
        if (inputs.isEmpty) {
          throw StateError('Missing image inputs for multimodal request.');
        }
        promptId = await scope.sendPromptWithImages(prompt, inputs);
      } else {
        promptId = await scope.sendPrompt(prompt);
      }

      StreamSubscription? subText;
      StreamSubscription? subDone;

      void cleanup() {
        subText?.cancel();
        subDone?.cancel();
        if (!controller.isClosed) controller.close();
        print('   ✅ Completed response for $userId');
      }

      subText = scope.stream.listen((token) {
        if (!controller.isClosed) controller.add(token);
      });

      subDone = scope.completions.listen((event) {
        if (event.promptId == promptId) {
          if (!event.success) {
            print('   ❌ Error: ${event.errorDetails}');
            if (!controller.isClosed)
              controller.addError(Exception(event.errorDetails));
          }
          cleanup();
        }
      });
    } catch (e, stack) {
      print('   🔥 Critical Error for $userId: $e');
      print(stack);
      if (!controller.isClosed) {
        controller.addError(e);
        controller.close();
      }
    }
  }

  /// Allocates a slot. If full, performs Soft Eviction (VRAM -> RAM).
  Future<LlamaScope> _allocateSlotWithEviction(String incomingUserId) async {
    // 1. Check Capacity
    if (_activeSessions.length >= config.maxSlots) {
      // 2. Find Victim (LRU)
      final oldestEntry = _lastUsed.entries
          .where((e) => _activeSessions
              .containsKey(e.key)) // Ensure we look at active ones
          .reduce((a, b) => a.value.isBefore(b.value) ? a : b);

      final victimId = oldestEntry.key;
      final victimScope = _activeSessions[victimId]!;

      print('   ⚠️  Slots full. Evicting $victimId to RAM...');

      try {
        // 3. SOFT EVICTION: Save State to RAM
        final stateData = await victimScope.saveState();
        _ramSessions[victimId] = stateData;
        print('      ↳ Saved ${stateData.lengthInBytes ~/ 1024} KB to RAM');
      } catch (e) {
        print('      ↳ ❌ Failed to save state, performing hard eviction: $e');
      }

      // 4. Dispose VRAM
      await victimScope.dispose();
      _activeSessions.remove(victimId);
    }

    // 5. Create new scope
    final parent = _parent;
    if (parent == null) {
      throw StateError('Text generation is disabled in embeddings-only mode.');
    }
    final scope = parent.getScope();
    _activeSessions[incomingUserId] = scope;
    return scope;
  }

  // --- TIER 3 LOGIC (Maintenance) ---

  /// Runs periodically to move old RAM sessions to Disk
  void _runMaintenance() {
    final now = DateTime.now();
    final List<String> toArchive = [];

    for (var entry in _ramSessions.entries) {
      final userId = entry.key;
      final lastSeen = _lastUsed[userId] ?? now;

      if (now.difference(lastSeen) > _ramTtl) {
        toArchive.add(userId);
      }
    }

    for (var userId in toArchive) {
      final path = _getDiskPath(userId);
      print('   📦 Archiving inactive user $userId from RAM to Disk: $path');
      try {
        final data = _ramSessions[userId]!;
        final file = File(_getDiskPath(userId));
        file.writeAsBytesSync(data);
        _ramSessions.remove(userId);
        print('      ↳ Archived successfully.');
      } catch (e) {
        print('      ↳ ❌ Archive failed: $e');
      }
    }
  }

  bool _diskSessionExists(String userId) {
    return File(_getDiskPath(userId)).existsSync();
  }

  String _getDiskPath(String userId) {
    final safeId = userId.replaceAll(RegExp(r'[^\w\-]'), '_');
    return p.join(_sessionsDir.path, '$safeId.bin');
  }

  Future<void> dispose() async {
    print('   💤 Shutting down engine...');
    _maintenanceTimer?.cancel();
    if (_embeddingsOnly) {
      await _embeddingsWorker?.dispose();
    } else {
      try {
        await _parent?.dispose().timeout(Duration(seconds: 5));
      } catch (e) {
        print('   ⚠️  Warning during engine disposal: $e');
      }
    }
    _activeSessions.clear();
    _ramSessions.clear();
  }
}

class ContextLostException implements Exception {
  final String message = "Context lost. Full history required.";
  @override
  String toString() => "ContextLostException: $message";
}
