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
    final isStateless = sessionId.startsWith('stateless-');

    // Attempt creation with RETRY logic
    // LlamaService can be finicky with slot allocation races, even if serialized.
    int retries = 0;
    const maxRetries = 3;
    bool created = false;

    while (!created && retries < maxRetries) {
      try {
        if (isStateless) {
          await (_service as dynamic).createSession(sessionId);
        } else {
          // Stateful handling (try create, ignore exists)
          try {
            await (_service as dynamic).createSession(sessionId);
          } catch (_) {}
        }
        created = true;
      } catch (e) {
        retries++;
        if (retries >= maxRetries) {
          // If it failed 3 times, we can't proceed.
          parentSendPort.send(LlamaResponse.error(
              "Session creation failed for $sessionId after $maxRetries retries: $e",
              promptId));
          return;
        }
        // Wait a bit before retrying to let slots free up
        await Future.delayed(Duration(milliseconds: 100 * retries));
      }
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

          // REMOVED deleteSession to prevent race conditions or state corruption.
          // Let LlamaService manage eviction naturally for now.
        },
        onError: (e) {
          _subscriptions.remove(promptId);
          parentSendPort.send(LlamaResponse.error(e.toString(), promptId));
          // REMOVED deleteSession
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
