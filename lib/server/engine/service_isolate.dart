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

  receivePort.listen((message) {
    if (message is LlamaCommand) {
      child.handle(message);
    }
  });
}

class ServiceChild {
  final SendPort parentSendPort;
  LlamaService? _service;

  // Track active subscriptions to cancel if needed
  final Map<String, StreamSubscription> _subscriptions = {};

  ServiceChild(this.parentSendPort);

  void handle(LlamaCommand command) {
    try {
      if (command is LlamaInit) {
        _handleInit(command);
      } else if (command is LlamaLoad) {
        _handleLoad(command);
      } else if (command is LlamaPrompt) {
        _handlePrompt(command);
      } else if (command is LlamaStop) {
        _handleStop(command);
      } else if (command is LlamaDispose) {
        _handleDispose();
      } else if (command is LlamaSaveState) {
        _handleSaveState(command);
      } else if (command is LlamaLoadState) {
        // Not supported by LlamaService (Memory persistence not implemented via this command yet)
        parentSendPort.send(
            LlamaResponse.error("LoadState not supported", command.slotId));
      } else if (command is LlamaLoadSession) {
        _handleLoadSession(command);
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

  void _handlePrompt(LlamaPrompt cmd) {
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
        },
        onError: (e) {
          _subscriptions.remove(promptId);
          parentSendPort.send(LlamaResponse.error(e.toString(), promptId));
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
