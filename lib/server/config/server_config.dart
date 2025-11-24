import 'package:llama_cpp_dart/llama_cpp_dart.dart';

class ServerConfig {
  // Server Settings
  final String host;
  final int port;
  final String? libraryPath;
  final String? apiKey;
  final int concurrency;
  final int maxSlots;
  final int timeoutSeconds;
  final String systemPrompt;

  // Engine ID
  final String name;
  final String modelPath;
  final String? mmprojPath;

  // Llama Parameters
  final ModelParams modelParams;
  final ContextParams contextParams;
  final SamplerParams samplerParams;
  final ChatFormat? chatFormat;

  ServerConfig({
    required this.name,
    required this.host,
    required this.port,
    this.libraryPath,
    this.apiKey,
    required this.concurrency,
    required this.maxSlots,
    required this.timeoutSeconds,
    required this.systemPrompt,
    required this.modelPath,
    this.mmprojPath,
    required this.modelParams,
    required this.contextParams,
    required this.samplerParams,
    this.chatFormat,
  });
}
