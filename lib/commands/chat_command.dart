import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:args/command_runner.dart';
import 'package:interact/interact.dart';
import 'package:path/path.dart' as p;

import 'session_repo.dart'; // Import your files
import 'chat_service.dart';

class ChatCommand extends Command {
  @override
  final String name = 'chat';
  @override
  final String description = 'Interactive AI workspace.';

  final _repo = SessionRepo();
  final _service = ChatService();

  ChatCommand() {
    addSubcommand(StartCommand(_repo, this));
    addSubcommand(ResumeCommand(_repo, this));
    addSubcommand(ListCommand(_repo));
  }

  @override
  Future<void> run() async {
    final rest = argResults?.rest ?? [];

    if (rest.isEmpty) {
      await _runWizard();
    } else {
      final arg = rest.first;
      if (_repo.exists(arg)) {
        await _startChatLoop(arg);
      } else {
        // Assume it's a config name
        final newId = await _repo.create(arg);
        await _startChatLoop(newId);
      }
    }
  }

  Future<void> _runWizard() async {
    final sessions = _repo.list();
    final choices = [
      '[ + ] Start New Chat...',
      ...sessions.map((s) => '${s.title} (${s.model})')
    ];

    final selection = Select(
      prompt: '🦅 Hugind Workspace',
      options: choices,
    ).interact();

    if (selection == 0) {
      final model = Input(prompt: 'Config Name:', defaultValue: 'my-assistant')
          .interact();
      final id = await _repo.create(model);
      await _startChatLoop(id);
    } else {
      await _startChatLoop(sessions[selection - 1].id);
    }
  }

  // Exposed for subcommands to use
  Future<void> _startChatLoop(String id) async {
    final session = await _repo.load(id);
    final messages = session['messages'] as List;
    final model = session['model'];

    print('\nLoaded Session: $id ($model)');
    _printContext(messages);

    // State for image attachment
    String? _pendingImage;

    // Trap Ctrl+C
    final sigint = ProcessSignal.sigint.watch().listen((_) async {
      print('\n❄️  Hibernating...');
      await _service.hibernate(id);
      exit(0);
    });

    try {
      while (true) {
        final prompt = _pendingImage != null ? '\n(🖼️ ) >>> ' : '\n>>> ';
        stdout.write(prompt);
        final input = stdin.readLineSync()?.trim();
        if (input == null) continue; // EOF
        if (input.isEmpty && _pendingImage == null) continue; // Empty enter

        // Slash Commands
        if (input.startsWith('/')) {
          if (input == '/exit' || input == '/quit') break;

          if (input.startsWith('/image')) {
            final args = input.split(' ');
            if (args.length < 2) {
              print('Usage: /image <path/to/image.jpg>');
              continue;
            }
            final path = args.sublist(1).join(' ');
            final file = File(path.trim());
            if (!file.existsSync()) {
              print('❌ File not found: $path');
              continue;
            }

            try {
              final bytes = await file.readAsBytes();
              final base64 = base64Encode(bytes);

              // Simple mime detection
              String mime = 'image/jpeg';
              final ext = p.extension(path).toLowerCase();
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

          // Add other commands here
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
        } else {
          userMsg = {'role': 'user', 'content': input};
        }

        // 1. Send (Optimistic + Retry Logic handled by Service)
        stdout.write('AI: ');
        final buffer = StringBuffer();

        try {
          final response = await _service.sendMessage(
              sessionId: id,
              model: model,
              fullHistory: messages, // Passed in case of 409
              newMessage: userMsg,
              isNewSession: messages.isEmpty);

          if (response.statusCode != 200) {
            print('Error ${response.statusCode}');
            continue;
          }

          final filter = _TokenStreamFilter();

          // 2. Stream & Parse
          await response.stream
              .transform(utf8.decoder)
              .transform(const LineSplitter())
              .listen((line) {
            if (line.startsWith('data: ')) {
              final data = line.substring(6);
              if (data == '[DONE]') return;
              try {
                final json = jsonDecode(data);
                final delta = json['choices'][0]['delta']['content'];
                if (delta != null) {
                  final safe = filter.process(delta);
                  if (safe.isNotEmpty) {
                    stdout.write(safe);
                    buffer.write(safe);
                  }
                }
              } catch (_) {}
            }
          }).asFuture();

          // Flush any remaining partial tokens (unless they were EOS)
          final remainder = filter.flush();
          if (remainder.isNotEmpty) {
            stdout.write(remainder);
            buffer.write(remainder);
          }

          print(''); // Newline

          // 3. Save to Local Disk (Thick Client)
          messages.add(userMsg);
          messages.add({'role': 'assistant', 'content': buffer.toString()});
          await _repo.save(id, session);
        } catch (e) {
          print('Connection Failed: $e');
        }
      }
    } finally {
      sigint.cancel();
      print('👋 Exiting...');
      await _service.hibernate(id);
    }
  }

  void _printContext(List messages) {
    if (messages.isEmpty) return;
    print('--- Recent Context ---');
    final start = (messages.length > 4) ? messages.length - 4 : 0;
    for (var i = start; i < messages.length; i++) {
      final m = messages[i];
      final role = m['role'].toString().toUpperCase();
      print('$role: ${(m['content'] as String).split('\n').first}...');
    }
    print('----------------------');
  }
}

// Minimal Subcommands delegating back to main logic
class StartCommand extends Command {
  final SessionRepo repo;
  final ChatCommand parentCmd; // To access _startChatLoop
  StartCommand(this.repo, this.parentCmd);
  @override
  String get name => 'start';
  @override
  String get description => 'Start new chat';
  @override
  Future<void> run() async {
    if (argResults!.rest.isEmpty)
      return print('Usage: hugind chat start <config>');
    final id = await repo.create(argResults!.rest.first);
    await parentCmd._startChatLoop(id);
  }
}

class ResumeCommand extends Command {
  final SessionRepo repo;
  final ChatCommand parentCmd;
  ResumeCommand(this.repo, this.parentCmd);
  @override
  String get name => 'resume';
  @override
  String get description => 'Resume chat ID';
  @override
  Future<void> run() async {
    if (argResults!.rest.isEmpty) {
      final sessions = repo.list();
      if (sessions.isEmpty) {
        print('No sessions found.');
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

      await parentCmd._startChatLoop(sessions[selection].id);
    } else {
      await parentCmd._startChatLoop(argResults!.rest.first);
    }
  }

  String _formatTime(DateTime d) {
    final diff = DateTime.now().difference(d);
    if (diff.inMinutes < 60) return '${diff.inMinutes}m ago';
    if (diff.inHours < 24) return '${diff.inHours}h ago';
    return '${diff.inDays}d ago';
  }
}

class ListCommand extends Command {
  final SessionRepo repo;
  ListCommand(this.repo);
  @override
  String get name => 'list';
  @override
  String get description => 'List sessions';
  @override
  void run() {
    print('ID              LAST ACTIVE   TITLE');
    for (var s in repo.list()) {
      print(
          '${s.id.padRight(14)}  ${_ago(s.lastActive).padRight(12)}  ${s.title}');
    }
  }

  String _ago(DateTime d) {
    final diff = DateTime.now().difference(d);
    return '${diff.inMinutes}m ago';
  }
}

class _TokenStreamFilter {
  final List<String> stopSequences = ['[EOS]', '<|endoftext|>', '<|im_end|>'];
  String _buffer = '';

  /// Returns the "safe" text to print, keeping potential partial stop tokens in buffer
  String process(String chunk) {
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
