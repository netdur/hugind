String tail(String s, int maxChars) {
  if (s.length <= maxChars) return s;
  return '... (truncated)\n' + s.substring(s.length - maxChars);
}

// Helper: strip ``` fences if model disobeys
String stripFences(String s) {
  String cleaned = s.trim();
  if (!cleaned.startsWith('```')) return cleaned;

  // remove first fence line
  final firstNl = cleaned.indexOf('\n');
  if (firstNl >= 0) cleaned = cleaned.substring(firstNl + 1);

  cleaned = cleaned.trim();
  if (cleaned.length >= 3) {
    final tail = cleaned.substring(cleaned.length - 3);
    if (tail == '```') {
      cleaned = cleaned.substring(0, cleaned.length - 3);
    }
  }

  return cleaned.trim();
}

// ignore: main_first_positional_parameter_type
dynamic main(Map<String, dynamic> context) async {
  final sys = context['capabilities']['sys'];
  final llm = context['capabilities']['llm'];

  // --- 1. SYSTEM CONTEXT ---
  String osInfo = 'POSIX';
  try {
    dynamic uname = await sys.run('uname', ['-a']);
    osInfo = uname.toString().trim();
  } catch (e) {}
  sys.print('💻 System: ' + osInfo);

  // Persist across the whole run (all user requests)
  String sessionHistory = '';

  while (true) {
    dynamic input = sys.readInput('\n💬 Request (or "exit"): ');
    final sInput = input.toString().trim();

    if (sInput == 'exit') {
      sys.print('👋 Goodbye.');
      break;
    }
    if (sInput.isEmpty) continue;

    // --- 2. REACT LOOP (per user request) ---
    int step = 0;
    String turnHistory = '';
    bool finished = false;

    while (step < 10 && !finished) {
      step += 1;
      sys.print('🔄 Step ' + step.toString() + '...');

      // Keep prompt context bounded
      final sessionCtx = tail(sessionHistory, 3500);
      final turnCtx = tail(turnHistory, 2500);

      // --- A. PROMPT (JSON Only) ---
      final prompt = '''SYSTEM:
You are a local CLI assistant running on the user's machine.
OS/ENV: $osInfo

USER REQUEST:
$sInput

SESSION HISTORY (previous user requests + important outcomes):
$sessionCtx

TURN HISTORY (this request: commands + outputs so far):
$turnCtx

HARD RULES (MUST FOLLOW):
1) Output MUST be ONLY a single JSON object. No markdown. No backticks. No code fences. No extra text.
2) Do NOT reveal chain-of-thought. Use "analysis" as a brief summary (1-2 sentences).
3) Propose at most ONE command per step.
4) Default to safe read-only commands. No sudo. No destructive actions (rm/mv/dd/overwrite) unless user explicitly requests.
5) Commands must be non-interactive. Avoid pagers/editors; add flags to disable them.
6) Use the history: do not repeat a command that already failed unless you change it meaningfully.
7) If you already have enough info, set done=true and provide answer; command must be empty.
8) If you cannot proceed safely, set done=true with an answer explaining what info is missing (and optionally add "needs").
9) When done=true, the "answer" field is MANDATORY. It must contain the final result or summary for the user.

OUTPUT JSON SCHEMA:
{
  "analysis": "brief summary only",
  "command": "string, empty if done=true",
  "done": false,
  "answer": "string, MANDATORY if done=true, empty if done=false",
  "needs": ["optional", "list", "of", "missing", "info"]
}''';

      dynamic response = await llm.chat(prompt);
      final respStr = response.toString();
      // sys.print('DEBUG: raw response=$respStr');

      // --- B. JSON PARSING ---
      dynamic action;
      String cleaned = stripFences(respStr);

      // best-effort: extract {...} if model adds extra text
      String jsonPart = cleaned;
      final jsonTrim = jsonPart.trim();
      if (!jsonTrim.startsWith('{')) {
        final start = jsonPart.indexOf('{');
        int end = -1;
        int i = jsonPart.length - 1;
        while (i >= 0) {
          if (jsonPart.substring(i, i + 1) == '}') {
            end = i;
            break;
          }
          i = i - 1;
        }
        if (start >= 0 && end > start) {
          jsonPart = jsonPart.substring(start, end + 1);
        }
      }

      try {
        action = jsonDecode(jsonPart);
      } catch (_) {
        action = null;
      }

      if (action == null || action is! Map) {
        sys.print('❌ Parse Error. Raw: ' + respStr);
        break;
      }

      final analysis = (action['analysis'] ?? '').toString();
      final command = (action['command'] ?? '').toString().trim();
      var answer = (action['answer'] ?? '').toString();
      final doneVal = action['done'];

      final isDone = (doneVal == true || doneVal.toString() == 'true');

      // Fallback: if done but no answer, use analysis
      if (isDone && answer.trim().isEmpty) {
        answer = analysis;
      }

      if (analysis.isNotEmpty) sys.print('🧠 ' + analysis);

      // --- C. ANSWER ---
      if (isDone && answer.trim().isNotEmpty) {
        sys.print('\n✨ ANSWER: ' + answer);
        // Store outcome in session history (important!)
        sessionHistory = sessionHistory +
            '\nUser: ' +
            sInput +
            '\nAnswer: ' +
            tail(answer.trim(), 600) +
            '\n';
        finished = true;
        break;
      }

      // --- D. COMMAND ---
      if (command.isEmpty) {
        sys.print('❌ Agent did not generate a command or an answer.');
        // Also store this failure; it helps future turns.
        sessionHistory = sessionHistory +
            '\nUser: ' +
            sInput +
            '\nAgentError: No command/answer at step ' +
            step.toString() +
            '.\n';
        break;
      }

      String safeCommand = command;
      if (safeCommand.startsWith('`'))
        safeCommand = safeCommand.replaceAll('`', '').trim();

      sys.print('👉 Action: ' + safeCommand);

      dynamic confirm = await sys.confirm('⚡ Run?');
      String output = '';
      if (confirm == true) {
        try {
          dynamic res = await sys.run('sh', ['-c', safeCommand + ' 2>&1']);
          output = res.toString();

          if (output.trim().isEmpty) output = '(No output)';
        } catch (e) {
          output = 'Error: ' + e.toString();
        }
      } else {
        output = 'User skipped this command.';
      }

      // Truncate for turn history (context window)
      String historyOut = output.trim();
      if (historyOut.length > 1200) {
        historyOut = historyOut.substring(0, 1200) + '... (truncated)';
      }

      // Update histories
      final stepRecord = 'Step ' +
          step.toString() +
          ':\nCommand: ' +
          safeCommand +
          '\nOutput:\n' +
          historyOut +
          '\n';

      turnHistory = turnHistory + '\n' + stepRecord;
      // For session history, keep it shorter and only keep important signals
      sessionHistory =
          sessionHistory + '\nUser: ' + sInput + '\n' + stepRecord + '\n';
      sessionHistory = tail(sessionHistory, 12000); // cap overall memory

      // Show first line to user (like you do)
      final outLines = output.split('\n');
      if (outLines.isNotEmpty) {
        sys.print('   Result: ' + outLines[0] + '...');
      }
    }
  }
}
