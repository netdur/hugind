import 'dart:async';
import 'dart:isolate';

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

  void send(LlamaCommand command) {
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
    if (message is LlamaCommand) {
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

  ServiceChild(this.parentSendPort);

  Future<void> handle(LlamaCommand command) async {
    try {
      if (command is LlamaInit) {
        _handleInit(command);
      } else if (command is LlamaLoad) {
        _handleLoad(command);
      } else if (command is LlamaPrompt) {
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

  void _handleLoad(LlamaLoad cmd) {
    try {
      _service = LlamaService(
        cmd.path,
        modelParams: cmd.modelParams,
        contextParams: cmd.contextParams,
        samplerParams: cmd.samplingParams,
        verbose: cmd.verbose,
        mmprojPath: cmd.mmprojPath,
      );

      parentSendPort.send(LlamaResponse.confirmation(LlamaStatus.ready));
    } catch (e) {
      parentSendPort.send(LlamaResponse.error("Load failed: $e"));
    }
  }

  Future<void> _handlePrompt(LlamaPrompt cmd) async {
    if (_service == null) {
      parentSendPort
          .send(LlamaResponse.error("Service not initialized", cmd.promptId));
      return;
    }

    final sessionId = cmd.slotId ?? 'default';
    final promptId = cmd.promptId;

    // Send "Generating" status immediately
    parentSendPort.send(LlamaResponse(
      text: "",
      isDone: false,
      status: LlamaStatus.generating,
      promptId: promptId,
    ));

    // Auto-create session if it's new (required by LlamaService)
    // We treat 'stateless-' sessions as ephemeral.
    final isStateless = sessionId.startsWith('stateless-');

    // Attempt creation. LlamaService usually ignores if already exists or handles it.
    // However, for stateless, we definitely want a new slot.
    // We cannot easily check existence without try-catch on create,
    // or assuming we must call it.
    // Based on error "Call createSession first", we MUST call it.
    try {
      // We might need to check if it exists? LlamaService.createSession might throw if exists?
      // Or we just try.
      // Given we don't have visibility into LlamaService source,
      // we'll try to create it. If it fails because it exists, we catch and proceed
      // (assuming it's reusable).
      // BUT for stateless, we expect it to NOT exist.
      if (isStateless) {
        // Dynamic cast to bypass analyzer if definition is stale
        await (_service as dynamic).createSession(sessionId);
      } else {
        // For stateful sessions, we also might need to create it if it's the first time.
        // We can't know for sure.
        // Ideally we should always try createSession -> catch "already exists".
        try {
          await (_service as dynamic).createSession(sessionId);
        } catch (_) {
          // Ignore "already exists" error
        }
      }
    } catch (e) {
      parentSendPort.send(LlamaResponse.error(
          "Session creation failed for $sessionId: $e", promptId));
      return;
    }

    Stream<String> stream;
    try {
      if (cmd.images != null && cmd.images!.isNotEmpty) {
        stream = _service!.generateWithMedia(
          sessionId,
          cmd.prompt,
          inputs: cmd.images!,
        );
      } else {
        stream = _service!.generateText(sessionId, prompt: cmd.prompt);
      }

      final sub = stream.listen(
        (token) {
          parentSendPort.send(LlamaResponse(
            text: token,
            isDone: false,
            status: LlamaStatus.generating,
            promptId: promptId,
          ));
        },
        onDone: () {
          _subscriptions.remove(promptId);
          parentSendPort.send(LlamaResponse(
            text: "",
            isDone: true,
            status: LlamaStatus.ready,
            promptId: promptId,
          ));

          if (isStateless) {
            try {
              (_service as dynamic).deleteSession(sessionId);
            } catch (e) {
              // log error?
            }
          }
        },
        onError: (e) {
          _subscriptions.remove(promptId);
          parentSendPort.send(LlamaResponse.error(e.toString(), promptId));

          if (isStateless) {
            try {
              (_service as dynamic).deleteSession(sessionId);
            } catch (_) {}
          }
        },
      );

      _subscriptions[promptId] = sub;
    } catch (e) {
      parentSendPort
          .send(LlamaResponse.error("Generation start failed: $e", promptId));
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
    await _service?.dispose();
    _service = null;
    parentSendPort.send(LlamaResponse.confirmation(LlamaStatus.disposed));
  }
}
