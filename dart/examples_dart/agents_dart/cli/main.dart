// --- HELPERS ---

/// Smartly truncates output to keep headers and the most recent data.
/// e.g., keeps first 5 lines (headers) and last 40 lines.
String smartTruncate(String text, int maxChars) {
  if (text.length <= maxChars) return text;

  final lines = text.split('\n');
  if (lines.length < 20) {
    // If few lines but long chars, just tail it
    return '... (truncated)\n' + text.substring(text.length - maxChars);
  }

  // Keep first 5 lines (headers) and last N lines to fit size
  final header = lines.take(5).join('\n');
  final remainingChars = maxChars - header.length - 50; // buffer

  if (remainingChars <= 0) return text.substring(text.length - maxChars);

  final tailStr = text.substring(text.length - remainingChars);
  // align tail to next newline
  final nextNl = tailStr.indexOf('\n');
  final cleanTail = (nextNl >= 0) ? tailStr.substring(nextNl + 1) : tailStr;

  return '$header\n\n... (middle content truncated) ...\n\n$cleanTail';
}

/// Aggressively cleans text to find the JSON object.
String extractJson(String text) {
  String cleaned = text.trim();
  // Remove markdown code blocks if present
  if (cleaned.contains('```')) {
    final start = cleaned.indexOf('```');
    final end = cleaned.lastIndexOf('```');
    if (end > start) {
      // Attempt to find the content inside the first block,
      // or just strip the fences.
      // Heuristic: find first '{' after first '```'
      final firstBrace = cleaned.indexOf('{', start);
      final lastBrace = cleaned.lastIndexOf('}', end);
      if (firstBrace != -1 && lastBrace != -1 && lastBrace > firstBrace) {
        cleaned = cleaned.substring(firstBrace, lastBrace + 1);
        return cleaned;
      }
    }
  }

  // Fallback: Find first '{' and last '}'
  final start = cleaned.indexOf('{');
  final end = cleaned.lastIndexOf('}');
  if (start >= 0 && end > start) {
    return cleaned.substring(start, end + 1);
  }
  return cleaned;
}

// --- MAIN AGENT ---

// ignore: main_first_positional_parameter_type
dynamic main(Map<String, dynamic> context) async {
  final sys = context['capabilities']['sys'];
  final llm = context['capabilities']['llm'];

  // 1. SETUP ENVIRONMENT
  String osInfo = 'Unknown POSIX';
  String cwd = '/';

  try {
    // Get OS Details
    final uname = await sys.run('uname', ['-sm']);
    osInfo = uname.toString().trim();
    // Get Current Directory
    final pwd = await sys.run('pwd', []);
    cwd = pwd.toString().trim();
  } catch (e) {
    sys.print('⚠️ Warning: Could not detect OS/CWD: $e');
  }

  sys.print('💻 System: $osInfo');
  sys.print('📂 CWD:    $cwd');

  // Long-term session memory
  String sessionHistory = '';

  while (true) {
    // 2. USER INPUT
    dynamic input = sys.readInput('\n💬 Request (or "exit"): ');
    final sInput = input.toString().trim();

    if (sInput == 'exit' || sInput == 'quit') {
      sys.print('👋 Goodbye.');
      break;
    }
    if (sInput.isEmpty) continue;

    // 3. REACT LOOP
    int step = 0;
    String turnHistory = '';
    bool finished = false;

    // Max steps per request to prevent infinite loops
    while (step < 12 && !finished) {
      step++;
      sys.print('🔄 Step $step...');

      // Update CWD for the prompt (in case previous step changed it via cd,
      // though cd usually doesn't persist in subprocesses, the Agent needs to know logic flow)
      // Note: In this architecture, actual 'cd' requires maintaining state,
      // but for CLI agents, we usually execute one-offs.
      // We will assume CWD stays constant unless we manually track it.

      // Prepare Context
      // Expanded limits: 10k chars for history, 5k for current turn
      final sessionCtx = smartTruncate(sessionHistory, 10000);
      final turnCtx = smartTruncate(turnHistory, 5000);

      // --- PROMPT ENGINEERING ---
      final prompt = '''SYSTEM:
You are a CLI expert on $osInfo.
Current Directory: $cwd

GOAL: Solve the user request efficiently.

RULES:
1. RESPONSE FORMAT: Output ONLY valid JSON. No Markdown. No Explanations outside JSON.
2. COMMANDS: 
   - Propose ONE command at a time.
   - macOS Optimization: Use `mdfind -name "x"` for fast search. Avoid `find /`.
   - Read-only commands (ls, cat, grep) are safe. Destructive commands require user confirm.
   - If you need to see a file, use `cat` or `head`.
3. PROCESS:
   - "analysis": Brief reasoning (1 sentence).
   - "command": The shell command to run. Empty if done.
   - "safe": true if read-only/harmless, false if side-effects.
   - "done": true if the answer is ready or task is impossible.
   - "answer": The final result for the user (MANDATORY if done=true).

JSON STRUCTURE:
{
  "analysis": "...",
  "command": "...",
  "safe": true/false,
  "done": true/false,
  "answer": "..."
}

HISTORY:
$sessionCtx

CURRENT TURN:
User: $sInput
$turnCtx
''';

      // --- LLM CALL & SELF-CORRECTION LOOP ---
      Map<String, dynamic> action = {};
      int retryCount = 0;
      String lastRawResponse = "";

      while (retryCount < 3) {
        String currentPrompt = prompt;

        // If retrying, append error instructions
        if (retryCount > 0) {
          currentPrompt +=
              '\n\nERROR: Last output was not valid JSON. \nRaw Output: $lastRawResponse\nFIX THE JSON SYNTAX.';
          sys.print('⚠️ JSON error. Retrying ($retryCount/3)...');
        }

        final response = await llm.chat(currentPrompt);
        lastRawResponse = response.toString();

        // Debug
        sys.print('###');
        sys.print('DEBUG: $lastRawResponse');
        sys.print('###');

        final jsonStr = extractJson(lastRawResponse);

        try {
          final decoded = jsonDecode(jsonStr);
          if (decoded is Map<String, dynamic>) {
            action = decoded;
            break; // Success
          }
        } catch (e) {
          // loop to retry
        }
        retryCount++;
      }

      // If still failed after retries
      if (action.isEmpty) {
        sys.print(
            '❌ Fatal: LLM failed to generate valid JSON after 3 attempts.');
        break;
      }

      // --- PARSE ACTION ---
      final analysis = action['analysis']?.toString() ?? '';
      final command = action['command']?.toString().trim() ?? '';
      var answer = action['answer']?.toString() ?? '';
      final isDone =
          action['done'] == true || action['done'].toString() == 'true';
      final isSafe =
          action['safe'] == true || action['safe'].toString() == 'true';

      if (analysis.isNotEmpty) sys.print('🧠 $analysis');

      // --- HANDLE FINISH ---
      if (isDone) {
        // Fallback if answer is empty
        if (answer.isEmpty) answer = analysis;

        sys.print('\n✨ ANSWER:\n$answer');

        // Update Session History
        sessionHistory += 'User: $sInput\nAnswer: $answer\n---\n';
        finished = true;
        break;
      }

      // --- HANDLE COMMAND ---
      if (command.isEmpty) {
        // Model set done=false but gave no command. Force a stop to avoid infinite loop.
        sys.print('❌ Error: Agent is thinking but provided no command.');
        break;
      }

      sys.print('👉 Action: $command');

      // Safety Check
      bool shouldRun = true;
      if (!isSafe) {
        final confirm = await sys.confirm('⚠️  Unsafe Command. Run?');
        shouldRun = (confirm == true);
      } else {
        sys.print('⚡ Auto-running safe command...');
      }

      String output = '';
      if (shouldRun) {
        try {
          // Run command, combine stdout/stderr
          final res = await sys.run('sh', ['-c', '$command 2>&1']);
          output = res.toString().trim();
          if (output.isEmpty) output = '(Command returned no output)';
        } catch (e) {
          output = 'Execution Error: $e';
        }
      } else {
        output = 'User skipped execution.';
      }

      // Truncate output for context
      // We use a larger buffer for the LLM to see
      final historyOut = smartTruncate(output, 2000);

      sys.print('   Result: ${output.split('\n').first}...');

      // Update Turn History
      turnHistory += '''
Step $step:
Cmd: $command
Out: $historyOut
''';
    }
  }
}
