import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:args/command_runner.dart';
import 'package:interact/interact.dart';
import 'package:path/path.dart' as p;
import 'package:yaml/yaml.dart';

import 'session_repo.dart'; // Import your files
import 'chat_service.dart';
import '../global_settings.dart';

class ChatCommand extends Command {
  @override
  final String name = 'chat';
  @override
  final String description = 'Interactive AI workspace.';

  final _repo = SessionRepo();
  final _service = ChatService();

  ChatCommand() {
    // We do NOT add subcommands via addSubcommand because CommandRunner
    // enforces usage of subcommands if they exist, preventing 'hugind chat'
    // from running the wizard. We handle dispatch manually in run().
  }

  @override
  Future<void> run() async {
    final rest = argResults?.rest ?? [];

    if (rest.isEmpty) {
      await _runWizard();
      return;
    }

    final sub = rest.first;
    final args = rest.skip(1).toList();

    switch (sub) {
      case 'start':
        await _runStart(args);
        break;
      case 'resume':
        await _runResume(args);
        break;
      case 'list':
        _runList();
        break;
      case 'delete':
        await _runDelete(args);
        break;
      case 'help':
        printUsage();
        break;
      default:
        // Fallback: Check if it's a session ID or config name directly?
        // Existing logic supported `hugind chat session_id`
        if (_repo.exists(sub)) {
          await _startChatLoop(sub);
        } else {
          // Assume it's a config name
          final newId = await _repo.create(sub);
          await _startChatLoop(newId);
        }
    }
  }

  // Manual Subcommand Handlers

  Future<void> _runStart(List<String> args) async {
    if (args.isEmpty) {
      await _wizardStartNew();
    } else {
      final id = await _repo.create(args.first);
      await _startChatLoop(id);
    }
  }

  Future<void> _runResume(List<String> args) async {
    if (args.isEmpty) {
      await _wizardResume();
    } else {
      await _startChatLoop(args.first);
    }
  }

  void _runList() {
    print('ID              LAST ACTIVE   TITLE');
    for (var s in _repo.list()) {
      print(
          '${s.id.padRight(14)}  ${_formatTime(s.lastActive).padRight(12)}  ${s.title}');
    }
  }

  @override
  void printUsage() {
    print('Usage: hugind chat [subcommand]');
    print('');
    print('Subcommands:');
    print('  start <config?>   Start new chat (interactive if no config)');
    print('  resume <id?>      Resume chat (interactive if no id)');
    print('  delete            Delete a chat session');
    print('  list              List sessions');
    print('');
    print('Run without arguments to launch the wizard.');
  }

  Future<void> _runWizard() async {
    final choices = ['Start New Chat', 'Resume Chat'];
    final selection = Select(
      prompt: '🦅 Hugind AI Workspace',
      options: choices,
    ).interact();

    if (selection == 0) {
      await _wizardStartNew();
    } else {
      await _wizardResume();
    }
  }

  Future<void> _wizardStartNew() async {
    final configs = _listConfigs();
    if (configs.isEmpty) {
      print('No configs found in ${_configHome()}/configs');
      final model = Input(
              prompt: 'Enter Model Name Manualy:', defaultValue: 'my-assistant')
          .interact();
      final id = await _repo.create(model);
      await _startChatLoop(id);
      return;
    }

    final choices = [...configs, 'Custom...'];
    final selection = Select(
      prompt: 'Select Configuration:',
      options: choices,
    ).interact();

    String model;
    if (selection == choices.length - 1) {
      model = Input(prompt: 'Enter Config Name:', defaultValue: 'my-assistant')
          .interact();
    } else {
      model = configs[selection];
    }
    final id = await _repo.create(model);
    await _startChatLoop(id);
  }

  Future<void> _wizardResume() async {
    final sessions = _repo.list();
    if (sessions.isEmpty) {
      print('No active sessions found.');
      // Fallback to start new
      if (Confirm(prompt: 'Start a new chat instead?').interact()) {
        await _wizardStartNew();
      }
      return;
    }

    final options = sessions.map((s) {
      final time = _formatTime(s.lastActive);
      return '${s.title} (${s.model}) - $time';
    }).toList();

    final selection = Select(
      prompt: 'Select a session to resume:',
      options: options,
    ).interact();

    await _startChatLoop(sessions[selection].id);
  }

  Future<void> _runDelete(List<String> args) async {
    final sessions = _repo.list();
    if (sessions.isEmpty) {
      print('No active sessions found.');
      return;
    }

    // Interactive selection if no ID provided in args?
    // User requested "hugind chat delete" -> select -> confirm
    final options = sessions.map((s) {
      final time = _formatTime(s.lastActive);
      return '${s.title} (${s.model}) - $time';
    }).toList();

    final selection = Select(
      prompt: 'Select a session to DELETE:',
      options: options,
    ).interact();

    final session = sessions[selection];
    final confirm = Confirm(
      prompt: 'Are you sure you want to delete "${session.title}"?',
      defaultValue: false,
    ).interact();

    if (confirm) {
      await _repo.delete(session.id);
      print('✅ Session deleted.');
    } else {
      print('❌ Operation cancelled.');
    }
  }

  List<String> _listConfigs() {
    final dir = Directory(p.join(_configHome(), 'configs'));
    if (!dir.existsSync()) return [];
    return dir
        .listSync()
        .whereType<File>()
        .where((f) => f.path.endsWith('.yml') || f.path.endsWith('.yaml'))
        .map((f) => p.basenameWithoutExtension(f.path))
        .toList();
  }

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

  String _formatTime(DateTime d) {
    final diff = DateTime.now().difference(d);
    if (diff.inMinutes < 60) return '${diff.inMinutes}m ago';
    if (diff.inHours < 24) return '${diff.inHours}h ago';
    return '${diff.inDays}d ago';
  }

  // Exposed for subcommands to use
  Future<void> _startChatLoop(String id) async {
    final session = await _repo.load(id);
    final messages = session['messages'] as List;
    final model = session['model'];
    final baseUrl = await _service.resolveBaseUrl(model.toString());

    // ANSI Colors
    const String cUser = '\x1B[32m'; // Green
    const String cAI = '\x1B[36m'; // Cyan
    const String cSys = '\x1B[90m'; // Dark Gray
    const String cReset = '\x1B[0m';
    const String cBold = '\x1B[1m';

    _printWelcome(id, model.toString());
    _printContext(messages, cUser, cAI, cSys, cReset);

    // State
    String? _pendingImage;
    String? _pendingText;

    // Trap Ctrl+C
    final sigint = ProcessSignal.sigint.watch().listen((_) async {
      print('\n\n❄️  Hibernating...');
      await _service.hibernate(id, baseUrl: baseUrl);
      exit(0);
    });

    try {
      while (true) {
        final prompt = _pendingImage != null
            ? '\n${cBold}🖼️  (Image) $cUser>>> $cReset'
            : _pendingText != null
                ? '\n${cBold}📄  (Text) $cUser>>> $cReset'
            : '\n$cUser>>> $cReset';

        stdout.write(prompt);
        final input = stdin.readLineSync()?.trim();

        if (input == null) break; // EOF
        if (input.isEmpty && _pendingImage == null) continue;

        // Slash Commands
        if (input.startsWith('/')) {
          final parts = input.split(' ');
          final cmd = parts[0].toLowerCase();
          final args = parts.skip(1).join(' ');

          if (cmd == '/exit' || cmd == '/quit') break;

          if (cmd == '/help') {
            print('''
${cBold}Available Commands:$cReset
  /image <path>   Attach an image to the next message
  /text <path>    Attach a text file to the next message
  /fork <name>    Save current session cache as a template
  /clear          Clear the terminal screen
  /exit, /quit    Exit the chat
             ''');
            continue;
          }

          if (cmd == '/clear') {
            print("\x1B[2J\x1B[0;0H");
            _printWelcome(id, model.toString());
            continue;
          }

          if (cmd == '/image') {
            if (args.isEmpty) {
              print('Usage: /image <path/to/image.jpg>');
              continue;
            }
            final file = File(args.trim());
            if (!file.existsSync()) {
              print('❌ File not found: ${file.path}');
              continue;
            }

            try {
              final bytes = await file.readAsBytes();
              final base64 = base64Encode(bytes);

              // Simple mime detection
              String mime = 'image/jpeg';
              final ext = p.extension(file.path).toLowerCase();
              if (ext == '.png')
                mime = 'image/png';
              else if (ext == '.webp') mime = 'image/webp';

              _pendingImage = 'data:$mime;base64,$base64';
              print('✅ Image attached! Type your message to send it.');
            } catch (e) {
              print('❌ Error reading file: $e');
            }
            continue;
          }

          if (cmd == '/fork') {
            final name = args.trim();
            if (name.isEmpty) {
              print('Usage: /fork <template-name>');
              continue;
            }
            if (p.basename(name) != name) {
              print('❌ Invalid template name. Use a simple name without paths.');
              continue;
            }
            try {
              await _service.hibernate(id, baseUrl: baseUrl);
              final sessionHome =
                  await _resolveSessionHome(model.toString());
              final src = File(p.join(sessionHome, '$id.bin'));
              if (!src.existsSync()) {
                print('❌ Session file not found: ${src.path}');
                continue;
              }
              final dest = File(p.join(sessionHome, '$name.bin'));
              dest.parent.createSync(recursive: true);
              src.copySync(dest.path);
              print('✅ Forked template saved: ${dest.path}');
            } catch (e) {
              print('❌ Fork failed: $e');
            }
            continue;
          }

          if (cmd == '/text') {
            if (args.isEmpty) {
              print('Usage: /text <path/to/text.txt>');
              continue;
            }
            final file = File(args.trim());
            if (!file.existsSync()) {
              print('❌ File not found: ${file.path}');
              continue;
            }

            try {
              final content = await file.readAsString();
              _pendingText = content;
              print('✅ Text attached! Type your message to send it.');
            } catch (e) {
              print('❌ Error reading file: $e');
            }
            continue;
          }

          print('Unknown command: $cmd');
          continue;
        }

        Map<String, dynamic> userMsg;
        if (_pendingImage != null) {
          userMsg = {
            'role': 'user',
            'content': [
              {
                'type': 'text',
                'text': input.isEmpty ? 'Describe this image' : input
              },
              {
                'type': 'image_url',
                'image_url': {'url': _pendingImage}
              }
            ]
          };
          _pendingImage = null; // Clear after sending
        } else if (_pendingText != null) {
          final merged =
              input.isEmpty ? _pendingText! : '$input\n\n$_pendingText';
          userMsg = {'role': 'user', 'content': merged};
          _pendingText = null; // Clear after sending
        } else {
          userMsg = {'role': 'user', 'content': input};
        }

        final buffer = StringBuffer();

        try {
          // Show spinner while waiting for connection/first-byte
          final spinner = _SimpleSpinner(message: 'Thinking...');
          spinner.start();

          final response = await _service.sendMessage(
              sessionId: id,
              model: model,
              fullHistory: messages,
              newMessage: userMsg,
              isNewSession: messages.isEmpty,
              baseUrl: baseUrl);

          if (response.statusCode != 200) {
            spinner.stop();
            print('Error ${response.statusCode}');
            continue;
          }

          final filter = _TokenStreamFilter();
          final highlighter = _SyntaxHighlighter();

          // 2. Stream & Parse
          final completer = Completer<void>();
          late final StreamSubscription<String> sub;

          bool isFirst = true;

          sub = response.stream
              .transform(utf8.decoder)
              .transform(const LineSplitter())
              .listen((line) {
            if (isFirst) {
              spinner.stop();
              isFirst = false;
              stdout.write(cAI); // Start AI Color
            }

            if (line.startsWith('data: ')) {
              final data = line.substring(6);
              if (data == '[DONE]') {
                sub.cancel();
                if (!completer.isCompleted) completer.complete();
                return;
              }
              try {
                final json = jsonDecode(data);
                final delta = json['choices'][0]['delta']['content'];
                if (delta != null) {
                  final safe = filter.process(delta);
                  if (safe.isNotEmpty) {
                    // Pass through highlighter
                    final formatted = highlighter.format(safe);
                    stdout.write(formatted);
                    buffer.write(safe);
                  }
                  if (filter.sawStop) {
                    sub.cancel();
                    if (!completer.isCompleted) completer.complete();
                  }
                }
              } catch (_) {}
            }
          }, onDone: () {
            if (isFirst) spinner.stop();
            if (!completer.isCompleted) completer.complete();
          }, onError: (_) {
            if (isFirst) spinner.stop();
            if (!completer.isCompleted) completer.complete();
          });

          await completer.future;

          // Flush
          final remainder = filter.flush();
          if (remainder.isNotEmpty) {
            stdout.write(highlighter.format(remainder));
            buffer.write(remainder);
          }

          stdout.write(cReset); // Reset color
          print(''); // Newline

          // 3. Save to Local Disk
          messages.add(userMsg);
          messages.add({'role': 'assistant', 'content': buffer.toString()});

          // Auto-Generate Title (1st to 5th turn)
          if (messages.length <= 10 && messages.length % 2 == 0) {
            stdout.write('${cSys}Generating title...$cReset');
            final title = await _service.generateTitle(
                model: model.toString(), history: messages, baseUrl: baseUrl);
            if (title.isNotEmpty) {
              session['title'] = title;
              stdout.write(
                  '\r${cSys}Title updated: $title             $cReset\n');
            } else {
              stdout.write('\r' + ' ' * 20 + '\r'); // clear line
            }
          }

          await _repo.save(id, session);
        } catch (e) {
          print('\nConnection Failed: $e');
        }
      }
    } finally {
      sigint.cancel();
      print('\n👋 Exiting...');
      await _service.hibernate(id, baseUrl: baseUrl);
    }
  }

  void _printWelcome(String id, String model) {
    const cTitle = '\x1B[1;34m';
    const cReset = '\x1B[0m';
    print('\n$cTitle🦅  HUGIND WORKSPACE$cReset');
    print('   Session: $id');
    print('   Model:   $model');
    print('   Type /help for commands.\n');
  }

  void _printContext(
      List messages, String cUser, String cAI, String cSys, String cReset) {
    if (messages.isEmpty) return;
    print('${cSys}--- Recent Context ---$cReset');
    final start = (messages.length > 6) ? messages.length - 6 : 0;
    for (var i = start; i < messages.length; i++) {
      final m = messages[i];
      final role = m['role'].toString().toLowerCase();
      final content = (m['content'] is String)
          ? m['content'] as String
          : '(multimodal content)';

      String prefix = '$cSys$role:$cReset';
      if (role == 'user') prefix = '$cUser$role:$cReset';
      if (role == 'assistant') prefix = '$cAI$role:$cReset';

      final preview = content.split('\n').take(1).join();
      print('$prefix $preview...');
    }
    print('${cSys}----------------------$cReset');
  }

  Future<String> _resolveSessionHome(String model) async {
    final fallback = await GlobalSettings.getSessionsPath();
    if (model.isEmpty) return fallback;

    final configDir = p.join(_configHome(), 'configs');
    final candidates = [
      p.join(configDir, '$model.yml'),
      p.join(configDir, '$model.yaml'),
    ];

    for (final path in candidates) {
      final file = File(path);
      if (!await file.exists()) continue;
      try {
        final content = await file.readAsString();
        final yaml = loadYaml(content);
        final server = yaml['server'] as Map? ?? {};
        final raw = server['session_home']?.toString();
        if (raw != null && raw.isNotEmpty) {
          return _resolvePathRelative(raw, path);
        }
      } catch (_) {
        return fallback;
      }
    }

    return fallback;
  }

  String _resolvePathRelative(String raw, String configPath) {
    if (p.isAbsolute(raw)) return raw;
    return p.normalize(p.join(p.dirname(configPath), raw));
  }
}

class _TokenStreamFilter {
  final List<String> stopSequences = ['[EOS]', '<|endoftext|>', '<|im_end|>'];
  String _buffer = '';
  bool sawStop = false;

  /// Returns the "safe" text to print, keeping potential partial stop tokens in buffer
  String process(String chunk) {
    sawStop = false;
    _buffer += chunk;

    // Quick check: if buffer doesn't look like it contains any stop seq, print all
    // Optimization: If buffer is very long, flush older parts
    if (_buffer.length > 50) {
      final safeLen = _buffer.length - 20;
      final safePart = _buffer.substring(0, safeLen);
      _buffer = _buffer.substring(safeLen);
      return safePart + _processBuffer();
    }

    return _processBuffer();
  }

  String _processBuffer() {
    String toPrint = '';

    // Check if buffer starts with a completed stop sequence
    for (final stop in stopSequences) {
      if (_buffer.contains(stop)) {
        // Found a stop token!
        // Print everything UP TO the stop token
        final index = _buffer.indexOf(stop);
        toPrint += _buffer.substring(0, index);
        sawStop = true;

        // Remove the stop token and everything before it from buffer effectively
        // Actually, we usually want to stop validation there, but here we just hide it.
        // We'll keep the remainder after the stop token in the buffer in case it's valid text?
        // Usually EOS means stop. So let's just swallow it.
        _buffer = _buffer.substring(index + stop.length);

        // Return recursively in case there are more tokens
        return toPrint + _processBuffer();
      }
    }

    // Check for PARTIAL match at the END of the buffer
    // e.g. Buffer: "Hello [EO"
    // We can print "Hello ", keep "[EO"

    int keepLength = 0;

    for (final stop in stopSequences) {
      // Check every suffix of buffer to see if it Matches a prefix of stop
      for (int i = 1; i < stop.length; i++) {
        if (i > _buffer.length) break;

        final suffix = _buffer.substring(_buffer.length - i);
        if (stop.startsWith(suffix)) {
          // This suffix COULD be the start of this stop token
          if (i > keepLength) keepLength = i;
        }
      }
    }

    if (keepLength > 0) {
      final splitPoint = _buffer.length - keepLength;
      toPrint += _buffer.substring(0, splitPoint);
      _buffer = _buffer.substring(splitPoint);
    } else {
      toPrint += _buffer;
      _buffer = '';
    }

    return toPrint;
  }

  String flush() {
    final ret =
        _buffer; // Print remainder logic? NO, if it's partial EOS we hide it.
    // Actually if we end with "[EO", and stream ends, it wasn't an EOS.
    // But for chat, safe to assume it's garbage or we just lose 2 chars.
    // Let's print it to be safe, unless it matches a stop sequence exactly.
    _buffer = '';
    for (final stop in stopSequences) {
      if (ret == stop) return '';
    }
    return ret;
  }
}

class _SyntaxHighlighter {
  // Simple heuristics for markdown
  // ```code``` -> color
  // **bold** -> bold
  // `inline` -> color

  bool insideBlock = false;
  String _buffer = '';

  String format(String chunk) {
    // This is a very naive streaming parser.
    // For robust highlighting, we need a full state machine.
    // We'll just highlight "```" toggles and paint content between them.

    final buffer = StringBuffer();
    final chars = chunk.split('');

    for (int i = 0; i < chars.length; i++) {
      final char = chars[i];
      _buffer += char;

      // Detect ```
      if (_buffer.endsWith('```')) {
        if (insideBlock) {
          // End of block
          insideBlock = false;
          // We need to backtrack to remove color from ```?
          // Simple: just print ``` then reset
          buffer.write('\x1B[0m'); // Reset
        } else {
          // Start of block
          insideBlock = true;
          buffer.write('\x1B[33m'); // Yellow for code
        }
        _buffer = ''; // Reset buffer after token match
      }

      buffer.write(char);
    }
    return buffer.toString();
  }
}

class _SimpleSpinner {
  Timer? _timer;
  int _frame = 0;
  final List<String> _frames = [
    '⠋',
    '⠙',
    '⠹',
    '⠸',
    '⠼',
    '⠴',
    '⠦',
    '⠧',
    '⠇',
    '⠏'
  ];
  final String message;

  _SimpleSpinner({required this.message});

  void start() {
    stdout.write('\x1B[?25l'); // Hide cursor
    _timer = Timer.periodic(const Duration(milliseconds: 80), (t) {
      final char = _frames[_frame % _frames.length];
      _frame++;
      stdout.write('\r\x1B[36m$char\x1B[0m $message');
    });
  }

  void stop() {
    _timer?.cancel();
    stdout.write('\x1B[?25h'); // Show cursor
    stdout.write('\r\x1B[2K\r'); // Clear line
  }
}
