import 'dart:async';
import 'dart:isolate';
import 'dart:io';

import 'package:llama_cpp_dart/llama_cpp_dart.dart';

class ServiceIsolate {
  final Isolate _isolate;
  final SendPort _sendPort;

  ServiceIsolate._(this._isolate, this._sendPort);

  static Future<ServiceIsolate> spawn(
    Function(LlamaResponse) onResponse, {
    String? libraryPath,
  }) async {
    final receivePort = ReceivePort();
    final isolate = await Isolate.spawn(
      _entryPoint,
      receivePort.sendPort,
      debugName: 'LlamaServiceIsolate',
    );

    final stream = receivePort.asBroadcastStream();
    final sendPortCompleter = Completer<SendPort>();

    stream.listen((message) {
      if (message is SendPort) {
        sendPortCompleter.complete(message);
      } else if (message is LlamaResponse) {
        onResponse(message);
      }
    });

    final sendPort = await sendPortCompleter.future;

    if (libraryPath != null) {
      sendPort.send(LlamaInit(libraryPath));
    }

    return ServiceIsolate._(isolate, sendPort);
  }

  void send(Object command) {
    _sendPort.send(command);
  }

  void dispose() {
    try {
      _sendPort.send(LlamaDispose());
    } catch (_) {}

    Future.delayed(const Duration(milliseconds: 200), () {
      _isolate.kill();
    });
  }
}

void _entryPoint(SendPort parentSendPort) {
  final receivePort = ReceivePort();
  parentSendPort.send(receivePort.sendPort);

  final child = ServiceChild(parentSendPort);

  // Serialize command processing to prevent race conditions during session creation/allocation
  Future<void> processingChain = Future.value();

  receivePort.listen((message) {
    if (message is LlamaCommand ||
        message is LlamaFreeSession ||
        message is LlamaPromptExtended ||
        message is LlamaLoadExtended) {
      // Chain the task
      processingChain = processingChain.then((_) async {
        await child.handle(message);
      }).catchError((e) {
        // Log error but keep chain alive.
        // Child.handle catches most internal errors, this is a fallback.
        // Assuming we can't easily log to file, we just suppress or send to parent.
        // parentSendPort.send(LlamaResponse.error("Isolate loop error: $e"));
      });
    }
  });
}

class ServiceChild {
  final SendPort parentSendPort;
  LlamaService? _service;

  // Track active subscriptions to cancel if needed
  final Map<String, StreamSubscription> _subscriptions = {};
  final Set<String> _knownSessions = {};

  ServiceChild(this.parentSendPort);

  Future<void> handle(dynamic command) async {
    try {
      if (command is LlamaInit) {
        _handleInit(command);
      } else if (command is LlamaLoadExtended || command is LlamaLoad) {
        _handleLoad(command);
      } else if (command is LlamaPrompt || command is LlamaPromptExtended) {
        await _handlePrompt(command);
      } else if (command is LlamaStop) {
        _handleStop(command);
      } else if (command is LlamaDispose) {
        await _handleDispose();
      } else if (command is LlamaSaveState) {
        await _handleSaveState(command);
      } else if (command is LlamaLoadState) {
        // Not supported by LlamaService (Memory persistence not implemented via this command yet)
        parentSendPort.send(
            LlamaResponse.error("LoadState not supported", command.slotId));
      } else if (command is LlamaLoadSession) {
        await _handleLoadSession(command);
      } else if (command is LlamaFreeSession) {
        _service?.freeSession(command.sessionId);
        // We could send confirmation, but free is usually fire-and-forget or sync-enough
        parentSendPort.send(
            LlamaResponse.confirmation(LlamaStatus.ready, command.sessionId));
      }
    } catch (e) {
      parentSendPort.send(LlamaResponse.error("Uncaught isolate error: $e"));
    }
  }

  Future<void> _handleSaveState(LlamaSaveState cmd) async {
    if (_service == null) {
      parentSendPort
          .send(LlamaResponse.error("Service not initialized", cmd.slotId));
      return;
    }
    // LlamaSaveState in isolate_types doesn't have path, so we use a convention.
    // LlamaService defaults sessionHome to './sessions'.
    try {
      final path = "${_service!.sessionHome}/${cmd.slotId}.bin";
      // Cast to dynamic because analyzer might see old void signature
      await (_service as dynamic).saveSession(cmd.slotId, path);
      // We use promptId to return the slotId for correlation
      parentSendPort
          .send(LlamaResponse.confirmation(LlamaStatus.ready, cmd.slotId));
    } catch (e) {
      parentSendPort.send(LlamaResponse.error("Save failed: $e", cmd.slotId));
    }
  }

  Future<void> _handleLoadSession(LlamaLoadSession cmd) async {
    if (_service == null) {
      parentSendPort
          .send(LlamaResponse.error("Service not initialized", cmd.slotId));
      return;
    }
    try {
      final success = await _service!.loadSession(cmd.slotId, cmd.path);
      if (success) {
        _knownSessions.add(cmd.slotId);
        parentSendPort
            .send(LlamaResponse.confirmation(LlamaStatus.ready, cmd.slotId));
      } else {
        parentSendPort
            .send(LlamaResponse.error("Load returned false", cmd.slotId));
      }
    } catch (e) {
      parentSendPort
          .send(LlamaResponse.error("Load session failed: $e", cmd.slotId));
    }
  }

  void _handleInit(LlamaInit cmd) {
    try {
      Llama.libraryPath = cmd.libraryPath;
      final _ = Llama.lib; // Force load
      parentSendPort
          .send(LlamaResponse.confirmation(LlamaStatus.uninitialized));
    } catch (e) {
      parentSendPort.send(LlamaResponse.error("Init failed: $e"));
    }
  }

  void _handleLoad(dynamic cmd) {
    try {
      final sessionHome = cmd is LlamaLoadExtended
          ? cmd.sessionHome
          : '${Directory.current.path}/sessions';
      _service = LlamaService(
        cmd.path,
        modelParams: cmd.modelParams,
        contextParams: cmd.contextParams,
        samplerParams: cmd.samplingParams,
        verbose: cmd.verbose,
        mmprojPath: cmd.mmprojPath,
        sessionHome: sessionHome, // Explicit session home
      );

      parentSendPort.send(LlamaResponse.confirmation(LlamaStatus.ready));
    } catch (e) {
      parentSendPort.send(LlamaResponse.error("Load failed: $e"));
    }
  }

  Future<void> _handlePrompt(dynamic cmd) async {
    // cmd can be LlamaPrompt (legacy/package) or LlamaPromptExtended (local)
    // Extract fields
    String prompt;
    String? promptId;
    List<LlamaImage>? images;
    String? slotId;
    bool clearHistory = true; // Default to true for standard command

    if (cmd is LlamaPromptExtended) {
      prompt = cmd.prompt;
      promptId = cmd.promptId;
      images = cmd.images;
      slotId = cmd.slotId;
      clearHistory = cmd.clearHistory;
    } else if (cmd is LlamaPrompt) {
      prompt = cmd.prompt;
      promptId = cmd.promptId;
      images = cmd.images;
      slotId = cmd.slotId;
    } else {
      return; // Should not happen
    }

    // Force string check
    if (promptId == null) return;
    final pid = promptId;

    if (_service == null) {
      parentSendPort.send(LlamaResponse.error("Service not initialized", pid));
      return;
    }

    final sessionId = slotId ?? 'default';

    // Send "Generating" status immediately
    parentSendPort.send(LlamaResponse(
      text: "",
      isDone: false,
      status: LlamaStatus.generating,
      promptId: pid,
    ));

    // Auto-create session if it's new (required by LlamaService)
    final isStateless = sessionId.startsWith('stateless-');
    final needsCreate = !_knownSessions.contains(sessionId);

    // With nSeqMax > 1 (Batching), we don't need complex retry logic for slots.
    // LlamaService internally manages slots based on nSeqMax.
    // We just ensure the session is created.
    if (needsCreate) {
      try {
        if (isStateless) {
          await (_service as dynamic).createSession(sessionId);
        } else {
          // Stateful handling (try create, ignore exists if it was implicitly created)
          try {
            await (_service as dynamic).createSession(sessionId);
          } catch (_) {}
        }
        _knownSessions.add(sessionId);
      } catch (e) {
        parentSendPort.send(LlamaResponse.error(
            "Session creation failed for $sessionId: $e", pid));
        return;
      }
    }

    Stream<String> stream;
    try {
      // Use the User's Pattern: setPrompt + generateText
      // This allows explicit control over clearHistory.

      if (images != null && images.isNotEmpty) {
        stream = _service!.generateWithMedia(
          sessionId,
          prompt,
          inputs: images,
        );
        // Media implies fresh context usually, or handled internally.
        // We subscribe immediately.
        final sub = stream.listen(
          (token) {
            parentSendPort.send(LlamaResponse(
              text: token,
              isDone: false,
              status: LlamaStatus.generating,
              promptId: pid,
            ));
          },
          onDone: () {
            _subscriptions.remove(pid);
            parentSendPort.send(LlamaResponse(
              text: "",
              isDone: true,
              status: LlamaStatus.ready,
              promptId: pid,
            ));
          },
          onError: (e) {
            _subscriptions.remove(pid);
            parentSendPort.send(LlamaResponse.error(e.toString(), pid));
          },
        );
        _subscriptions[pid] = sub;
        _waitForCompletion(sessionId, pid);
      } else {
        // Text Only: Use the explicit control pattern
        // 1. Get Stream
        stream = _service!.generateText(sessionId);

        // 2. Subscribe FIRST (Matching user example)
        final sub = stream.listen(
          (token) {
            parentSendPort.send(LlamaResponse(
              text: token,
              isDone: false,
              status: LlamaStatus.generating,
              promptId: pid,
            ));
          },
          onDone: () {
            _subscriptions.remove(pid);
            parentSendPort.send(LlamaResponse(
              text: "",
              isDone: true,
              status: LlamaStatus.ready,
              promptId: pid,
            ));
          },
          onError: (e) {
            _subscriptions.remove(pid);
            parentSendPort.send(LlamaResponse.error(e.toString(), pid));
          },
        );
        _subscriptions[pid] = sub;

        // 3. Set Prompt (Triggers generation)
        await _service!
            .setPrompt(sessionId, prompt, clearHistory: clearHistory);
        _waitForCompletion(sessionId, pid);
      }
    } catch (e) {
      parentSendPort
          .send(LlamaResponse.error("Generation start failed: $e", pid));
    }
  }

  void _handleStop(LlamaStop cmd) {
    // Unsupported
  }

  Future<void> _handleDispose() async {
    for (final sub in _subscriptions.values) {
      await sub.cancel();
    }
    _subscriptions.clear();
    _knownSessions.clear();
    await _service?.dispose();
    _service = null;
    parentSendPort.send(LlamaResponse.confirmation(LlamaStatus.disposed));
  }

  Future<void> _waitForCompletion(String sessionId, String promptId) async {
    try {
      while (true) {
        if (_service == null) break;
        final status = (_service as dynamic).status(sessionId);
        if (status != LlamaStatus.generating) break;
        await Future.delayed(const Duration(milliseconds: 10));
      }
    } catch (_) {
      // If status isn't available, fall back to stream completion only.
      return;
    }

    if (!_subscriptions.containsKey(promptId)) return;
    await _subscriptions[promptId]!.cancel();
    _subscriptions.remove(promptId);
    parentSendPort.send(LlamaResponse(
      text: "",
      isDone: true,
      status: LlamaStatus.ready,
      promptId: promptId,
    ));
  }
}

class LlamaFreeSession {
  final String sessionId;
  LlamaFreeSession(this.sessionId);
}

class LlamaPromptExtended {
  final String prompt;
  final String? promptId;
  final List<LlamaImage>? images;
  final String? slotId;
  final bool clearHistory;

  LlamaPromptExtended(
    this.prompt,
    this.promptId, {
    this.images,
    this.slotId,
    this.clearHistory = true,
  });
}

class LlamaLoadExtended {
  final String path;
  final ModelParams modelParams;
  final ContextParams contextParams;
  final SamplerParams samplingParams;
  final bool verbose;
  final String? mmprojPath;
  final String sessionHome;

  LlamaLoadExtended({
    required this.path,
    required this.modelParams,
    required this.contextParams,
    required this.samplingParams,
    required this.sessionHome,
    this.verbose = false,
    this.mmprojPath,
  });
}
