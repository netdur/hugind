import 'dart:async';
import 'dart:io';
import 'package:llama_cpp_dart/llama_cpp_dart.dart';
import '../config/server_config.dart';
import 'embedding_worker.dart';
import 'service_isolate.dart';

class LlamaEngine {
  final ServerConfig config;
  final bool _embeddingsOnly;

  ServiceIsolate? _isolate;
  final EmbeddingsWorker? _embeddingsWorker;

  // Session ID -> Completer for current generation
  // ignore: unused_field
  final Map<String, Completer<void>> _activeGenerations = {};

  // Stream Controllers for response distribution
  // promptId -> Controller
  final Map<String, StreamController<String>> _promptStreams = {};
  final Map<String, Completer<void>> _promptCompleters = {};

  LlamaEngine(this.config)
      : _embeddingsOnly = config.embeddingsEnabled,
        _embeddingsWorker = config.embeddingsEnabled
            ? EmbeddingsWorker.fromConfig(config)
            : null;

  Future<void> init() async {
    if (_embeddingsOnly) {
      await _embeddingsWorker?.start();
      return;
    }

    _isolate = await ServiceIsolate.spawn(
      _handleResponse,
      libraryPath: config.libraryPath ?? Llama.libraryPath,
    );

    // Send Load Command
    // We wrap this in a completer to wait for 'ready'
    final readyCompleter = Completer<void>();
    _readyWaiters.add(readyCompleter);

    _isolate!.send(LlamaLoadExtended(
      path: config.modelPath,
      modelParams: config.modelParams,
      contextParams: config.contextParams
        ..nSeqMax = config.maxSlots, // Use max slots from config
      samplingParams: config.samplerParams,
      verbose: true, // debug
      mmprojPath: config.mmprojPath,
      sessionHome: config.sessionHome,
    ));

    await readyCompleter.future;
    print('   ✨ LlamaService Engine Ready: ${config.modelPath}');
  }

  final List<Completer<void>> _readyWaiters = [];

  void _handleResponse(LlamaResponse r) {
    if (r.status == LlamaStatus.uninitialized) {
      // Init done, waiting for load...
    } else if (r.status == LlamaStatus.ready && r.isConfirmation) {
      // Load done OR other confirm
      for (final c in _readyWaiters) {
        if (!c.isCompleted) c.complete();
      }
      _readyWaiters.clear();
    } else if (r.status == LlamaStatus.error) {
      print("   ❌ LlamaService Error: ${r.errorDetails}");
      // Propagate to relevant stream if promptId present via r.promptId
      if (r.promptId != null && _promptStreams.containsKey(r.promptId)) {
        _promptStreams[r.promptId]!.addError(Exception(r.errorDetails));
      }
    } else if (r.status == LlamaStatus.generating) {
      if (r.promptId != null && _promptStreams.containsKey(r.promptId)) {
        if (r.text.isNotEmpty) {
          _promptStreams[r.promptId]!.add(r.text);
        }
      }
    }

    if (r.isDone && r.promptId != null) {
      _finishPrompt(r.promptId!);
    }
  }

  void _finishPrompt(String promptId) {
    if (_promptStreams.containsKey(promptId)) {
      _promptStreams[promptId]!.close();
      _promptStreams.remove(promptId);
    }
    if (_promptCompleters.containsKey(promptId)) {
      _promptCompleters[promptId]!.complete();
      _promptCompleters.remove(promptId);
    }

    // Auto-Save state for stateful sessions
    // Derive userId from promptId (format: ${userId}_timestamp)
    final parts = promptId.split('_');
    if (parts.isNotEmpty) {
      // userId might contain underscores, so we join all but last
      final userId = parts.sublist(0, parts.length - 1).join('_');
      if (userId.startsWith('stateless-')) {
        _isolate!.send(LlamaFreeSession(userId));
      } else {
        _isolate!.send(LlamaSaveState(userId));
      }
    }
  }

  Future<List<double>> embed(String input) async {
    if (!config.embeddingsEnabled || _embeddingsWorker == null) {
      throw StateError('Embeddings not enabled');
    }
    return _embeddingsWorker!.embed(input);
  }

  Stream<String> generateStream(String userId, List<Message> messages,
      {bool isFreshSession = true}) {
    if (_embeddingsOnly) throw StateError("Embeddings only mode");

    // Prepare prompt
    final format = config.chatFormat ?? ChatFormat.chatml;
    final history = ChatHistory();
    for (var m in messages)
      history.addMessage(role: m.role, content: m.content, images: m.images);

    // If new session or explicit fresh, we might want to ensure system prompt.
    // LlamaService manages state.
    // For simplicity, we assume LlamaService appends (chat mode).
    // If isFreshSession is true, in legacy we cleared.
    // In LlamaService, we might need a way to clear?
    // LlamaService doesn't expose Clear via Isolate yet easily (LlamaClear is global).
    // user 'clear' logic is tricky in multi-user service.
    // For now, we just append. (Limitation of this refactor without clear-session support).

    if (isFreshSession) {
      if (history.messages.isEmpty ||
          history.messages.first.role != Role.system) {
        history.messages.insert(
            0, Message(role: Role.system, content: config.systemPrompt));
      }
    }

    final promptId = "${userId}_${DateTime.now().millisecondsSinceEpoch}";

    // Auto-Restore check (Simple convention: ${config.sessionHome}/$userId.bin)
    if (!isFreshSession) {
      final sessionFile = File("${config.sessionHome}/$userId.bin");
      if (sessionFile.existsSync()) {
        // Explicitly load session state
        _isolate!.send(LlamaLoadSession(userId, sessionFile.path));
      }
    }

    // Check for images
    final hasImages = history.images.isNotEmpty;
    String prompt;
    List<LlamaImage>? inputs;

    if (hasImages) {
      final exported =
          history.exportWithMedia(format, leaveLastAssistantOpen: true);
      prompt = exported.$1;
      final rawInputs = exported.$2;
      inputs = rawInputs.whereType<LlamaImage>().toList();
    } else {
      prompt = history.exportFormat(format, leaveLastAssistantOpen: true);
    }

    final controller = StreamController<String>();
    _promptStreams[promptId] = controller;

    // Send
    // Send
    _isolate!.send(LlamaPromptExtended(
      prompt,
      promptId,
      images: inputs,
      slotId: userId, // Mapping UserID to SlotID/SessionID
      clearHistory: isFreshSession, // CRITICAL: Control history clearing
    ));

    return controller.stream;
  }

  Future<bool> hibernateSession(String userId) async {
    if (_embeddingsOnly) return false;
    // Confirm hibernation
    _isolate!.send(LlamaSaveState(userId));
    // We assume success for the command fire.
    //Ideally we should track completion, but this signature returns bool instantly.
    return true;
  }

  Future<void> freeSession(String userId) async {
    if (_embeddingsOnly) return;
    _isolate!.send(LlamaFreeSession(userId));
  }

  Future<void> dispose() async {
    print('   💤 Shutting down engine...');
    if (_embeddingsOnly) {
      await _embeddingsWorker?.dispose();
    } else {
      _isolate?.dispose();
    }
  }
}

class ContextLostException implements Exception {
  final String message = "Context lost. Full history required.";
  @override
  String toString() => "ContextLostException: $message";
}
