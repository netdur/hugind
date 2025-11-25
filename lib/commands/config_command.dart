import 'dart:convert';
import 'dart:io';
import 'dart:math' as math;

import 'package:args/command_runner.dart';
import 'package:interact/interact.dart';
import 'package:path/path.dart' as p;

import '../global_settings.dart';
import '../repo_manager.dart';

class ConfigCommand extends Command {
  @override
  final String name = 'config';
  @override
  final String description = 'Configuration utilities (init, list, check).';

  ConfigCommand() {
    addSubcommand(InfoCommand());
    addSubcommand(InitCommand());
    addSubcommand(ListConfigsCommand());
    addSubcommand(RemoveConfigCommand());
    addSubcommand(DefaultsCommand());
  }
}

// =============================================================================
// 1. INFO COMMAND
// =============================================================================
class InfoCommand extends Command {
  @override
  final String name = 'info';
  @override
  final String description = 'Show detected hardware details.';

  @override
  Future<void> run() async {
    final inspector = _SystemInspector();
    final info = await inspector.inspect();

    print('System Information');
    print('------------------');
    print('OS: ${info.os}');
    print('Arch: ${info.arch}');
    print('CPU: ${info.cpuModel ?? 'Unknown'}');
    print(
        'Cores: ${info.physicalCores} physical / ${info.logicalCores} logical');
    print('Memory: ${_formatMemory(info.memoryBytes)}');
    print(
        'Disk: ${_formatMemory(info.diskTotalBytes)} total / ${_formatMemory(info.diskAvailableBytes)} free');

    if (info.gpus.isEmpty) {
      print('GPUs: None detected');
    } else {
      print('GPUs:');
      for (final gpu in info.gpus) {
        final mem = gpu.memory ?? 'Unknown VRAM';
        print('  - ${gpu.name} ($mem)');
      }
    }

    final preset = _recommendPreset(info);
    print('\nRecommendation: $preset');
  }
}

// =============================================================================
// 2. INIT COMMAND (The Wizard)
// =============================================================================
class InitCommand extends Command {
  @override
  final String name = 'init';
  @override
  final String description =
      'Generate a config by merging base template with hardware presets.';

  InitCommand() {
    argParser.addOption('model', abbr: 'm', help: 'Path to the model file');
  }

  @override
  Future<void> run() async {
    if (argResults!.rest.isEmpty) {
      print('Usage: hugind config init <name>');
      return;
    }

    final configName = argResults!.rest.first;

    // --- A. Probe Hardware ---
    print('Probing hardware... (this may take a moment)');
    final inspector = _SystemInspector();
    final info = await inspector.inspect();
    final recommendedPreset = _recommendPreset(info);

    _printHardwareSummary(info, recommendedPreset);

    // --- B. Choose Preset ---
    final presets = ['metal_unified', 'cuda_dedicated', 'cpu_only'];
    final defaultIndex =
        recommendedPreset == null ? 0 : presets.indexOf(recommendedPreset);

    final chosenIndex = Select(
      prompt: 'Choose a hardware preset to apply',
      options: presets,
      initialIndex: defaultIndex < 0 ? 0 : defaultIndex,
    ).interact();
    final chosenPresetName = presets[chosenIndex];

    // --- C. Load Templates ---
    String baseContent;
    String presetContent;
    try {
      baseContent = await _loadTemplate('config.yml');
      presetContent = await _loadTemplate('$chosenPresetName.yml');
    } catch (e) {
      print('\n❌ Critical Error loading templates: $e');
      return;
    }

    // --- D. Select Main Model ---
    String modelPath = argResults?['model'] ?? '';
    if (modelPath.isEmpty) {
      modelPath = await _interactiveModelSelection();
    }
    if (modelPath.isEmpty) {
      if (Confirm(
              prompt: 'No model selected. Enter path manually?',
              defaultValue: false)
          .interact()) {
        modelPath = Input(prompt: 'Path to .gguf file:').interact();
      }
    }

    // --- E. Select Helpers ---
    String mmprojPath = '';
    if (modelPath.isNotEmpty &&
        !modelPath.startsWith('@') &&
        File(modelPath).existsSync()) {
      mmprojPath = _detectAndSelectSiblings(
          modelPath: modelPath,
          keywords: ['mmproj', 'projector', 'vision'],
          label: 'Vision Projector');
    }

    // --- F. Chat Format ---
    final chatFormats = ['auto', 'chatml', 'gemma', 'alpaca', 'harmony'];
    String detectedFormat = 'auto';
    final mLower = p.basename(modelPath).toLowerCase();
    if (mLower.contains('gemma'))
      detectedFormat = 'gemma';
    else if (mLower.contains('llama-3'))
      detectedFormat = 'alpaca';
    else if (mLower.contains('qwen') ||
        mLower.contains('yi') ||
        mLower.contains('smol')) detectedFormat = 'chatml';

    final chatFormatIndex = Select(
      prompt: 'Select Chat Format Template',
      options: chatFormats,
      initialIndex: chatFormats.indexOf(detectedFormat) != -1
          ? chatFormats.indexOf(detectedFormat)
          : 0,
    ).interact();
    final chosenChatFormat = chatFormats[chatFormatIndex];

    // --- G. Memory & Context ---
    int recommendedCtx = 4096;
    if (modelPath.isNotEmpty &&
        File(modelPath).existsSync() &&
        info.memoryBytes != null) {
      final fileSize = File(modelPath).lengthSync();
      final sysRam = info.memoryBytes!;
      print('\n🧠 Memory Analysis:');
      print('  System RAM: ${_formatMemory(sysRam)}');
      print('  Model Size: ${_formatMemory(fileSize)}');

      final availableForKV = sysRam - fileSize - (2 * 1024 * 1024 * 1024);
      if (availableForKV <= 0) {
        print('  ⚠️  Warning: Low memory. Restricting context.');
        recommendedCtx = 2048;
      } else {
        final maxSafeTokens = availableForKV ~/ (1024 * 1024);
        print('  Est. Max Context: ~$maxSafeTokens tokens');
        if (maxSafeTokens >= 32768)
          recommendedCtx = 32768;
        else if (maxSafeTokens >= 16384)
          recommendedCtx = 16384;
        else if (maxSafeTokens >= 8192)
          recommendedCtx = 8192;
        else
          recommendedCtx = 4096;
      }
    }

    final ctxOptions = [2048, 4096, 8192, 16384, 32768, 65536];
    final ctxLabels = ctxOptions
        .map((c) => c == recommendedCtx ? '$c (Recommended)' : '$c')
        .toList();
    final ctxIndex = Select(
      prompt: 'Select Context Size (Ctx)',
      options: ctxLabels,
      initialIndex: ctxOptions.indexOf(recommendedCtx) != -1
          ? ctxOptions.indexOf(recommendedCtx)
          : 1,
    ).interact();
    final finalCtx = ctxOptions[ctxIndex];

    // --- H. Detect Library Path (THE FIX) ---
    // 1. Check Global Settings first (Override)
    String? resolvedLibPath = await GlobalSettings.getLibraryPath();

    // 2. If Global is empty, Auto-Detect relative to Executable
    if (resolvedLibPath == null || resolvedLibPath.isEmpty) {
      resolvedLibPath = _detectLibraryPath();
    }

    // --- I. Merge & Replace ---
    var finalContent = baseContent;

    // 1. Preset Overrides
    _extractAndMerge(presetContent, (key, value) {
      finalContent = _replaceValue(finalContent, key, value);
    });

    // 2. Paths
    final shortModel = _shortenPath(modelPath);
    finalContent = _replaceValue(finalContent, 'path',
        shortModel.isNotEmpty ? '"$shortModel"' : '"@PLACEHOLDER"');

    if (mmprojPath.isNotEmpty) {
      finalContent = _replaceValue(
          finalContent, 'mmproj_path', '"${_shortenPath(mmprojPath)}"');
    }

    // 3. Library Path Injection (THE FIX)
    // We write the ACTUAL detected path into the YAML
    if (resolvedLibPath.isNotEmpty) {
      finalContent =
          _replaceValue(finalContent, 'library_path', '"$resolvedLibPath"');
    }

    // 4. Chat & Context
    finalContent = _replaceValue(finalContent, 'format', chosenChatFormat);
    finalContent = _replaceValue(finalContent, 'size', finalCtx.toString());

    // 5. Threads
    int threads = (chosenPresetName == 'cuda_dedicated')
        ? 4
        : math.max(1, info.physicalCores - 2);
    finalContent = _replaceValue(finalContent, 'threads', threads.toString());
    finalContent =
        _replaceValue(finalContent, 'threads_batch', threads.toString());

    // --- J. Write to Disk ---
    final destDir = Directory(p.join(_configHome(), 'configs'));
    if (!await destDir.exists()) await destDir.create(recursive: true);
    final destFile = File(p.join(destDir.path, '$configName.yml'));

    if (await destFile.exists()) {
      if (!Confirm(
              prompt: 'Config "$configName" exists. Overwrite?',
              defaultValue: false)
          .interact()) {
        return;
      }
    }

    await destFile.writeAsString(finalContent);

    print('\n✔ Config written to ${destFile.path}');
    print('  • Preset: $chosenPresetName');
    print('  • Model: $shortModel');
    print('  • Library: $resolvedLibPath'); // Show user exactly what we found
    print('  • Context: $finalCtx');
  }

  /// Looks for the library relative to the running executable
  String _detectLibraryPath() {
    final binDir = File(Platform.resolvedExecutable).parent.path;

    // Priority list: mtmd (ours) -> llama (standard)
    final candidates = [
      p.join(binDir, 'libmtmd.dylib'),
      p.join(binDir, 'libllama.dylib'),
      p.join(binDir, 'libllama.so'),
      p.join(binDir, 'llama.dll'),
    ];

    for (final path in candidates) {
      if (File(path).existsSync()) {
        return path;
      }
    }

    // If not found, return empty string (let user handle it manually)
    return '';
  }

  // --- Template Loader ---
  Future<String> _loadTemplate(String filename) async {
    final scriptDir = p.dirname(Platform.script.toFilePath());
    final exeDir = p.dirname(Platform.resolvedExecutable);
    final home = Platform.environment['HOME'] ?? '';

    // Prioritized list of locations
    final candidates = <String>[
      // 1. Dev: Relative to script
      p.join(scriptDir, 'config', filename),
      p.join(scriptDir, '../bin/config', filename),

      // 2. Dist: Relative to executable
      p.join(exeDir, 'config', filename),

      // 3. Homebrew (Apple Silicon)
      '/opt/homebrew/share/hugind/config/$filename',

      // 4. Linux / Homebrew (Intel)
      '/usr/local/share/hugind/config/$filename',

      // 5. User Local
      p.join(home, '.local/share/hugind/config', filename),
    ];

    for (final path in candidates) {
      final file = File(path);
      if (await file.exists()) {
        return file.readAsString();
      }
    }

    throw Exception('Configuration template "$filename" not found.\n'
        'Checked locations:\n${candidates.map((c) => " - $c").join("\n")}');
  }

  // --- Helpers ---

  String _shortenPath(String path) {
    if (path.isEmpty) return '';
    final env = Platform.environment;
    final home = env['HOME'] ?? env['USERPROFILE'];

    if (home != null && path.startsWith(home)) {
      if (path == home) return '~';
      final cleanHome = home.endsWith(p.separator)
          ? home.substring(0, home.length - 1)
          : home;
      if (path.startsWith('$cleanHome${p.separator}')) {
        return path.replaceFirst(cleanHome, '~');
      }
    }
    return path;
  }

  // FIXED: Robust Regex Replacement preserving comments and spacing
  String _replaceValue(String content, String key, String newValue) {
    // 1. (^|\n)(\s*)      -> Start of line + indent (Group 1, 2)
    // 2. (key:)           -> Key (Group 3)
    // 3. \s+              -> Whitespace separator
    // 4. (.*?)            -> Old Value (Group 4)
    // 5. (\s*#.*)?$       -> Comment (Group 5)
    final regex = RegExp(
        r'(^|\n)(\s*)(' + RegExp.escape(key) + r':)\s+(.*?)(\s*#.*)?$',
        multiLine: true);

    if (!regex.hasMatch(content)) return content;

    return content.replaceAllMapped(regex, (match) {
      final newline = match.group(1) ?? '';
      final indent = match.group(2) ?? '';
      final keyPart = match.group(3) ?? '';
      // Group 4 is old value (ignored)
      final comment = match.group(5) ?? '';

      String suffix = '';
      if (comment.isNotEmpty) {
        // Ensure proper spacing before the comment
        suffix = '  ${comment.trim()}';
      }

      return '$newline$indent$keyPart $newValue$suffix';
    });
  }

  void _extractAndMerge(
      String presetYaml, Function(String k, String v) onFound) {
    final lines = LineSplitter.split(presetYaml);
    for (var line in lines) {
      line = line.trim();
      if (line.startsWith('#') || line.isEmpty) continue;
      if (!line.contains(':')) continue;

      final parts = line.split(':');
      final key = parts[0].trim();
      var value = parts.sublist(1).join(':').trim();

      // Strip inline comments from preset values too
      if (value.contains('#')) value = value.split('#')[0].trim();

      if (value.isNotEmpty) onFound(key, value);
    }
  }

  Future<String> _interactiveModelSelection() async {
    final manager = RepoManager();
    List<String> repos = [];
    try {
      repos = await manager.listRepos();
    } catch (e) {
      return '';
    }

    if (repos.isEmpty) return '';

    final repoIndex = Select(
      prompt: 'Select a Model Repository',
      options: repos,
    ).interact();
    final selectedRepo = repos[repoIndex];

    List<File> files = [];
    try {
      files = await manager.getLocalFiles(selectedRepo);
    } catch (e) {
      return '';
    }

    final validFiles =
        files.where((f) => f.path.toLowerCase().endsWith('.gguf')).toList();
    if (validFiles.isEmpty) {
      print('No .gguf files found in $selectedRepo');
      return '';
    }

    final fileOptions = validFiles.map((f) => p.basename(f.path)).toList();
    final fileIndex = Select(
      prompt: 'Select the Model File',
      options: fileOptions,
    ).interact();

    return validFiles[fileIndex].path;
  }

  String _detectAndSelectSiblings({
    required String modelPath,
    required List<String> keywords,
    required String label,
  }) {
    final file = File(modelPath);
    if (!file.existsSync()) return '';

    try {
      final dir = file.parent;
      final candidates = dir.listSync().whereType<File>().where((f) {
        final name = p.basename(f.path).toLowerCase();
        final isMain = f.path == modelPath;
        final isGguf = name.endsWith('.gguf');
        final matchesKeyword = keywords.any((k) => name.contains(k));
        return !isMain && isGguf && matchesKeyword;
      }).toList();

      if (candidates.isEmpty) return '';

      if (candidates.length == 1) {
        final found = candidates.first.path;
        print('  ✨ Auto-detected $label: ${p.basename(found)}');
        return found;
      }

      print('\n🔎 Multiple candidates found for $label:');
      final options = candidates.map((f) => p.basename(f.path)).toList();
      options.add('Skip / None');

      final index = Select(
        prompt: 'Select which $label to use',
        options: options,
      ).interact();

      if (index < candidates.length) {
        return candidates[index].path;
      }
    } catch (e) {
      // ignore permissions
    }
    return '';
  }
}

// =============================================================================
// 3. LIST COMMAND
// =============================================================================
class ListConfigsCommand extends Command {
  @override
  final String name = 'list';
  @override
  final String description = 'List saved configs.';

  @override
  Future<void> run() async {
    final dir = Directory(p.join(_configHome(), 'configs'));
    if (!await dir.exists()) {
      print('No configs found.');
      return;
    }
    final entries = await dir
        .list()
        .where((e) =>
            e is File && (e.path.endsWith('.yml') || e.path.endsWith('.yaml')))
        .map((e) => p.basenameWithoutExtension(e.path))
        .toList();

    if (entries.isEmpty) {
      print('No configs found.');
      return;
    }
    print('Saved Configs:');
    for (final name in entries) print('- $name');
  }
}

// =============================================================================
// 4. REMOVE COMMAND
// =============================================================================
class RemoveConfigCommand extends Command {
  @override
  final String name = 'remove';
  @override
  final String description = 'Delete a saved config.';

  @override
  Future<void> run() async {
    if (argResults!.rest.isEmpty) {
      print('Usage: hugind config remove <name>');
      return;
    }
    final name = argResults!.rest.first;
    final file = File(p.join(_configHome(), 'configs', '$name.yml'));
    if (!await file.exists()) {
      print('Config "$name" not found.');
      return;
    }
    if (Confirm(prompt: 'Delete config "$name"?', defaultValue: false)
        .interact()) {
      await file.delete();
      print('Deleted.');
    }
  }
}

// =============================================================================
// 5. DEFAULTS COMMAND
// =============================================================================
class DefaultsCommand extends Command {
  @override
  final String name = 'defaults';
  @override
  final String description = 'Set global defaults (library path, API keys).';

  DefaultsCommand() {
    argParser.addOption('lib',
        help: 'Set the default path to libllama.dylib/so');
    argParser.addOption('hf-token', help: 'Set Hugging Face User Access Token');
  }

  @override
  Future<void> run() async {
    if (argResults!.arguments.isEmpty) {
      // Print current defaults
      final settings = await GlobalSettings.load();
      print('\nGlobal Settings (~/.hugind/settings.yml):');
      print('-' * 40);
      if (settings.isEmpty) print('No defaults set.');
      settings.forEach((k, v) => print('$k: $v'));
      print('-' * 40);
      print('\nUsage:');
      print('  hugind config defaults --lib /path/to/libllama.dylib');
      print('  hugind config defaults --hf-token hf_xxxxxx');
      return;
    }

    if (argResults!['lib'] != null) {
      final path = argResults!['lib'];
      if (!File(path).existsSync()) {
        print('⚠️  Warning: File does not exist at $path');
      }
      await GlobalSettings.set('library_path', path);
      print('✅ Global Library Path updated.');
    }

    if (argResults!['hf-token'] != null) {
      await GlobalSettings.set('hf_token', argResults!['hf-token']);
      print('✅ Global Hugging Face Token updated.');
    }
  }
}

// =============================================================================
// UTILITIES & HARDWARE PROBE
// =============================================================================

String _configHome() {
  final env = Platform.environment;
  if (Platform.isWindows) {
    final appData = env['APPDATA'];
    if (appData != null) return p.join(appData, 'hugind');
    return p.join(env['USERPROFILE'] ?? '.', '.hugind');
  }
  final xdg = env['XDG_CONFIG_HOME'];
  if (xdg != null && xdg.isNotEmpty) return p.join(xdg, 'hugind');
  return p.join(env['HOME'] ?? '.', '.hugind');
}

String _formatMemory(int? bytes) {
  if (bytes == null) return 'Unknown';
  final gb = bytes / (1024 * 1024 * 1024);
  return '${gb.toStringAsFixed(1)} GB';
}

void _printHardwareSummary(SystemInfo info, String? recommendation) {
  print('System probe complete:');
  print(
      '  CPU: ${info.cpuModel ?? 'Unknown'} (${info.physicalCores}c/${info.logicalCores}t)');
  print('  Memory: ${_formatMemory(info.memoryBytes)}');
  print('  GPUs: ${_gpuSummary(info)}');
  if (recommendation != null) print('Recommended preset: $recommendation');
}

String _gpuSummary(SystemInfo info) {
  if (info.gpus.isEmpty) return 'none';
  return info.gpus
      .map((g) => g.memory == null ? g.name : '${g.name} (${g.memory})')
      .join(', ');
}

String? _recommendPreset(SystemInfo info) {
  if (info.gpus.any((g) => g.name.toLowerCase().contains('nvidia')))
    return 'cuda_dedicated';
  if (info.gpus.any((g) => g.name.toLowerCase().contains('apple')))
    return 'metal_unified';
  return 'cpu_only';
}

class SystemInfo {
  final String os;
  final String arch;
  final String? cpuModel;
  final int logicalCores;
  final int physicalCores;
  final int? memoryBytes;
  final int? diskTotalBytes;
  final int? diskAvailableBytes;
  final List<GpuInfo> gpus;

  SystemInfo({
    required this.os,
    required this.arch,
    required this.cpuModel,
    required this.logicalCores,
    required this.physicalCores,
    required this.memoryBytes,
    required this.diskTotalBytes,
    required this.diskAvailableBytes,
    required this.gpus,
  });
}

class GpuInfo {
  final String name;
  final String? memory;
  GpuInfo({required this.name, this.memory});
}

class _SystemInspector {
  Future<SystemInfo> inspect() async {
    final os = '${Platform.operatingSystem} ${Platform.operatingSystemVersion}';
    final arch = (await _run('uname', ['-m']))?.trim() ?? 'unknown';
    final cpuModel = await _cpuModel();
    final logical = Platform.numberOfProcessors;
    final physical = await _physicalCores() ?? logical;
    final memoryBytes = await _memoryBytes();
    final disk = await _diskSpace();
    final gpus = await _detectGpus();

    return SystemInfo(
      os: os,
      arch: arch,
      cpuModel: cpuModel,
      logicalCores: logical,
      physicalCores: physical,
      memoryBytes: memoryBytes,
      diskTotalBytes: disk.$1,
      diskAvailableBytes: disk.$2,
      gpus: gpus,
    );
  }

  Future<String?> _cpuModel() async {
    if (Platform.isMacOS)
      return (await _run('sysctl', ['-n', 'machdep.cpu.brand_string']))?.trim();
    if (Platform.isLinux) {
      final res = await _run('lscpu', []);
      if (res != null) {
        for (final l in res.split('\n'))
          if (l.toLowerCase().startsWith('model name'))
            return l.split(':').last.trim();
      }
    }
    if (Platform.isWindows) {
      final res = await _run('wmic', ['cpu', 'get', 'name']);
      if (res != null) {
        final lines =
            res.split('\n').where((l) => l.trim().isNotEmpty).toList();
        if (lines.length > 1) return lines[1].trim();
      }
    }
    return null;
  }

  Future<int?> _physicalCores() async {
    if (Platform.isMacOS) {
      final res = await _run('sysctl', ['-n', 'hw.physicalcpu']);
      return res != null ? int.tryParse(res.trim()) : null;
    }
    if (Platform.isLinux) {
      final res = await _run('nproc', ['--all']);
      return res != null ? int.tryParse(res.trim()) : null;
    }
    if (Platform.isWindows) {
      final res = await _run('wmic', ['cpu', 'get', 'NumberOfCores']);
      if (res != null) {
        final lines =
            res.split('\n').where((l) => l.trim().isNotEmpty).toList();
        if (lines.length > 1) return int.tryParse(lines[1].trim());
      }
    }
    return null;
  }

  Future<int?> _memoryBytes() async {
    if (Platform.isMacOS) {
      final res = await _run('sysctl', ['-n', 'hw.memsize']);
      return res != null ? int.tryParse(res.trim()) : null;
    }
    if (Platform.isLinux) {
      final file = File('/proc/meminfo');
      if (await file.exists()) {
        final c = await file.readAsString();
        final m = RegExp(r'MemTotal:\s+(\d+)\s+kB').firstMatch(c);
        if (m != null) return int.parse(m.group(1)!) * 1024;
      }
    }
    if (Platform.isWindows) {
      final res =
          await _run('wmic', ['ComputerSystem', 'get', 'TotalPhysicalMemory']);
      if (res != null) {
        final lines =
            res.split('\n').where((l) => l.trim().isNotEmpty).toList();
        if (lines.length > 1) return int.tryParse(lines[1].trim());
      }
    }
    return null;
  }

  Future<List<GpuInfo>> _detectGpus() async {
    final gpus = <GpuInfo>[];

    // 1. Try NVIDIA-SMI
    try {
      final nvidia = await _run('nvidia-smi',
          ['--query-gpu=name,memory.total', '--format=csv,noheader']);
      if (nvidia != null && nvidia.trim().isNotEmpty) {
        for (final line in nvidia.split('\n')) {
          if (line.trim().isEmpty) continue;
          final parts = line.split(',');
          gpus.add(GpuInfo(
              name: parts[0].trim(),
              memory: parts.length > 1 ? parts[1].trim() : null));
        }
      }
    } catch (e) {/* ignore */}

    if (gpus.isNotEmpty) return gpus;

    // 2. Platform Fallbacks
    if (Platform.isMacOS) {
      final profiler = await _run('system_profiler', ['SPDisplaysDataType']);
      if (profiler != null) {
        for (final match
            in RegExp(r'Chipset Model:\s+(.*)').allMatches(profiler)) {
          gpus.add(GpuInfo(name: match.group(1)!.trim(), memory: null));
        }
      }
    } else if (Platform.isWindows) {
      final res = await _run(
          'wmic', ['path', 'win32_VideoController', 'get', 'name,adapterram']);
      if (res != null) {
        final lines =
            res.split('\n').where((l) => l.trim().isNotEmpty).toList();
        for (var i = 1; i < lines.length; i++) {
          gpus.add(GpuInfo(name: lines[i].trim(), memory: null));
        }
      }
    }

    return gpus;
  }

  Future<(int?, int?)> _diskSpace() async {
    if (Platform.isWindows) {
      return (null, null);
    }
    final res = await _run('df', ['-k', '/']);
    if (res == null) return (null, null);
    final lines = res.split('\n');
    if (lines.length < 2) return (null, null);
    final parts =
        lines[1].split(RegExp(r'\s+')).where((s) => s.isNotEmpty).toList();
    if (parts.length >= 4) {
      final t = int.tryParse(parts[1]);
      final a = int.tryParse(parts[3]);
      return (t != null ? t * 1024 : null, a != null ? a * 1024 : null);
    }
    return (null, null);
  }

  Future<String?> _run(String cmd, List<String> args) async {
    try {
      final res =
          await Process.run(cmd, args).timeout(const Duration(seconds: 2));
      return res.exitCode == 0 ? res.stdout.toString() : null;
    } catch (e) {
      return null;
    }
  }
}
