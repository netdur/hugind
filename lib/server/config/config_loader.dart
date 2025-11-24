import 'dart:io';
import 'package:path/path.dart' as p;
import 'package:yaml/yaml.dart';
import 'package:llama_cpp_dart/llama_cpp_dart.dart';
import 'server_config.dart';

class ConfigLoader {
  static Future<ServerConfig> load(String configPath) async {
    final file = File(configPath);
    if (!await file.exists()) {
      throw Exception("Config file not found: $configPath");
    }

    // 1. Read & Parse YAML
    final content = await file.readAsString();
    final yaml = loadYaml(content);
    final name = p.basenameWithoutExtension(configPath);

    // 2. Server Section
    final server = yaml['server'] ?? {};
    final host = server['host'] ?? '0.0.0.0';
    final port = server['port'] ?? 8080;
    final apiKey = server['api_key']?.toString().isEmpty == true
        ? null
        : server['api_key'];

    // NEW: Parse Library Path
    String? libPath;
    if (server['library_path'] != null &&
        server['library_path'].toString().isNotEmpty) {
      libPath = _resolvePath(server['library_path']);
    }

    // Load System Prompt
    String systemPrompt = "You are a helpful assistant.";
    if (server['system_prompt_file'] != null) {
      systemPrompt =
          await _loadSystemPrompt(server['system_prompt_file'], configPath);
    }

    // 3. Model Section & Validation
    final model = yaml['model'] ?? {};
    final modelPath = _resolvePath(model['path']);

    // --- VALIDATION CHECK 1: Model Existence ---
    if (modelPath.isEmpty || !await File(modelPath).exists()) {
      throw Exception(
          "Model file not found at: $modelPath\nRun 'hugind model list' to verify.");
    }

    final mmprojPath = model['mmproj_path'] != null
        ? _resolvePath(model['mmproj_path'])
        : null;
    if (mmprojPath != null && !await File(mmprojPath).exists()) {
      print(
          "⚠️  Warning: Vision projector not found at $mmprojPath. Vision will be disabled.");
    }

    // 4. Construct Parameters
    final modelParams = ModelParams()
      ..nGpuLayers = model['gpu_layers'] ?? 99
      ..splitMode = _parseSplitMode(model['split_mode'])
      ..mainGpu = model['main_gpu'] ?? 0
      ..useMemorymap = model['use_mmap'] ?? true
      ..useMemoryLock = model['use_mlock'] ?? false
      ..vocabOnly = model['vocab_only'] ?? false;

    final context = yaml['context'] ?? {};
    final contextParams = ContextParams()
      ..nCtx = context['size'] ?? 4096
      ..nBatch = context['batch_size'] ?? 2048
      ..nUbatch = context['ubatch_size'] ?? 512
      ..nThreads = context['threads'] ?? 8
      ..nThreadsBatch = context['threads_batch'] ?? 8
      ..flashAttention = _parseFlashAttn(context['flash_attention'])
      ..typeK = _parseCacheType(context['cache_type_k'])
      ..typeV = _parseCacheType(context['cache_type_v'])
      ..offloadKqv = context['offload_kqv'] ?? true;

    final sampling = yaml['sampling'] ?? {};
    final samplerParams = SamplerParams()
      ..temp = (sampling['temp'] ?? 0.7).toDouble()
      ..topK = sampling['top_k'] ?? 40
      ..topP = (sampling['top_p'] ?? 0.95).toDouble()
      ..minP = (sampling['min_p'] ?? 0.05).toDouble()
      ..minP = (sampling['min_p'] ?? 0.05).toDouble()
      ..dryMultiplier = (sampling['dry_multiplier'] ?? 0.0).toDouble();

    // --- NEW: Parse Chat Section ---
    final chat = yaml['chat'] as Map? ?? {};
    final formatStr = chat['format']?.toString().toLowerCase();

    ChatFormat? chatFormat;
    if (formatStr != null && formatStr != 'auto') {
      try {
        // Match string "gemma" to ChatFormat.gemma
        chatFormat = ChatFormat.values.firstWhere((e) => e.name == formatStr);
      } catch (_) {
        print(
            "⚠️ Warning: Unknown chat format '$formatStr'. Using auto-detection.");
      }
    }

    return ServerConfig(
      name: name,
      host: host,
      port: port,
      libraryPath: libPath,
      apiKey: apiKey,
      concurrency: server['concurrency'] ?? 1,
      maxSlots: server['max_slots'] ?? 4,
      timeoutSeconds: server['timeout_seconds'] ?? 600,
      systemPrompt: systemPrompt,
      modelPath: modelPath,
      mmprojPath: mmprojPath,
      modelParams: modelParams,
      contextParams: contextParams,
      samplerParams: samplerParams,
      chatFormat: chatFormat,
    );
  }

  // --- Helpers ---

  static String _resolvePath(String? rawPath) {
    if (rawPath == null || rawPath.isEmpty) return '';
    String path = rawPath;
    if (path.startsWith('~')) {
      final home =
          Platform.environment['HOME'] ?? Platform.environment['USERPROFILE'];
      if (home != null) path = path.replaceFirst('~', home);
    }
    return p.normalize(p.absolute(path));
  }

  static Future<String> _loadSystemPrompt(
      String promptPath, String configPath) async {
    String resolvedPath = _resolvePath(promptPath);
    // If relative, make it relative to config file
    if (!p.isAbsolute(resolvedPath) && !promptPath.startsWith('~')) {
      resolvedPath = p.join(p.dirname(configPath), promptPath);
    }

    final file = File(resolvedPath);
    if (await file.exists()) return (await file.readAsString()).trim();
    return "You are a helpful assistant.";
  }

  static LlamaSplitMode _parseSplitMode(String? val) {
    return LlamaSplitMode.values.firstWhere((e) => e.name == val?.toLowerCase(),
        orElse: () => LlamaSplitMode.layer);
  }

  static LlamaFlashAttnType _parseFlashAttn(dynamic val) {
    if (val == true ||
        val.toString() == 'true' ||
        val.toString() == 'enabled') {
      return LlamaFlashAttnType.enabled;
    }
    return LlamaFlashAttnType.disabled;
  }

  static LlamaKvCacheType _parseCacheType(String? val) {
    return LlamaKvCacheType.values.firstWhere(
        (e) => e.name == val?.toLowerCase(),
        orElse: () => LlamaKvCacheType.f16);
  }
}
