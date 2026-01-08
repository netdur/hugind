import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:args/command_runner.dart';
import 'package:interact/interact.dart';

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

    // Trap Ctrl+C
    final sigint = ProcessSignal.sigint.watch().listen((_) async {
      print('\n❄️  Hibernating...');
      await _service.hibernate(id);
      exit(0);
    });

    try {
      while (true) {
        stdout.write('\n>>> ');
        final input = stdin.readLineSync()?.trim();
        if (input == null || input.isEmpty) continue;

        // Slash Commands
        if (input.startsWith('/')) {
          if (input == '/exit' || input == '/quit') break;
          // Add other commands here
          continue;
        }

        final userMsg = {'role': 'user', 'content': input};

        // 1. Send (Optimistic + Retry Logic handled by Service)
        stdout.write('AI: ');
        final buffer = StringBuffer();

        try {
          final response = await _service.sendMessage(
              sessionId: id,
              model: model,
              fullHistory: messages, // Passed in case of 409
              newMessage: userMsg);

          if (response.statusCode != 200) {
            print('Error ${response.statusCode}');
            continue;
          }

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
                  stdout.write(delta);
                  buffer.write(delta);
                }
              } catch (_) {}
            }
          }).asFuture();

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
    if (argResults!.rest.isEmpty)
      return print('Usage: hugind chat resume <id>');
    await parentCmd._startChatLoop(argResults!.rest.first);
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
