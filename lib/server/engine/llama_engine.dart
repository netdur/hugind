import 'dart:async';
import 'dart:io';
import 'dart:typed_data';
import 'package:path/path.dart' as p;
import 'package:llama_cpp_dart/llama_cpp_dart.dart';
import '../config/server_config.dart';

class LlamaEngine {
  final ServerConfig config;
  final LlamaParent _parent;

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

  LlamaEngine(this.config)
      : _parent = LlamaParent(LlamaLoad(
          path: config.modelPath,
          modelParams: config.modelParams,
          contextParams: config.contextParams,
          samplingParams: config.samplerParams,
          mmprojPath: config.mmprojPath,
        ));

  Future<void> init() async {
    await _parent.init();

    // Ensure session directory exists
    final dir = Directory('sessions');
    if (!await dir.exists()) {
      await dir.create();
    }

    // Start background maintenance loop (Every 1 minute)
    _maintenanceTimer = Timer.periodic(Duration(minutes: 1), (timer) {
      _runMaintenance();
    });
  }

  Stream<String> generateStream(String userId, List<Message> messages) {
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

    _executeGeneration(userId, messages, controller, scopeCompleter);
    return controller.stream;
  }

  Future<void> _executeGeneration(
    String userId,
    List<Message> messages,
    StreamController<String> controller,
    Completer<LlamaScope> scopeCompleter,
  ) async {
    try {
      LlamaScope scope;
      bool isNewSession = false;
      bool restoredFromState = false;

      // ---------------------------------------------------------
      // 1. RESOLVE SESSION (Tier 1 -> Tier 2 -> Tier 3 -> New)
      // ---------------------------------------------------------

      if (_activeSessions.containsKey(userId)) {
        // [TIER 1] HOT HIT: User is already in VRAM
        scope = _activeSessions[userId]!;
        print('   🔥 [Tier 1] VRAM Hit: $userId (Slot: ${scope.id})');
      } else if (_ramSessions.containsKey(userId)) {
        // [TIER 2] WARM HIT: User is in RAM, needs a VRAM slot
        print('   🧊 [Tier 2] RAM Hit: $userId. Restoring...');
        scope = await _allocateSlotWithEviction(userId);

        final stateData = _ramSessions[userId]!;
        await scope.loadState(stateData); // Restore memory
        _ramSessions.remove(userId);
        restoredFromState = true;
      } else if (_diskSessionExists(userId)) {
        // [TIER 3] COLD HIT: User is on Disk, needs a VRAM slot
        print('   💾 [Tier 3] Disk Hit: $userId. Loading from file...');
        scope = await _allocateSlotWithEviction(userId);

        await scope.loadSession(_getDiskPath(userId));
        restoredFromState = true;
      } else {
        // [NEW] No history found
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
      ChatFormat format;
      if (config.chatFormat != null) {
        format = config.chatFormat!;
      } else {
        format = _detectFormat(config.modelPath);
      }

      final history = ChatHistory();
      for (var m in messages)
        history.addMessage(role: m.role, content: m.content);

      String prompt;

      if (isNewSession || restoredFromState) {
        if (history.messages.isEmpty ||
            history.messages.first.role != Role.system) {
          history.messages.insert(
              0, Message(role: Role.system, content: config.systemPrompt));
        }
        prompt = history.exportFormat(format, leaveLastAssistantOpen: true);
      } else {
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
      final promptId = await scope.sendPrompt(prompt);

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
    final scope = _parent.getScope();
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
      print('   📦 Archiving inactive user $userId from RAM to Disk...');
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
    return p.join('sessions', '$safeId.bin');
  }

  Future<void> dispose() async {
    print('   💤 Shutting down engine...');
    _maintenanceTimer?.cancel();
    await _parent.dispose();
    _activeSessions.clear();
    _ramSessions.clear();
  }

  ChatFormat _detectFormat(String path) {
    final p = path.toLowerCase();
    if (p.contains('gemma') || p.contains('smol')) return ChatFormat.gemma;
    if (p.contains('llama-3') || p.contains('alpaca')) return ChatFormat.alpaca;
    return ChatFormat.chatml;
  }
}
