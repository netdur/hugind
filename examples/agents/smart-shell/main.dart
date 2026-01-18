// ignore: main_first_positional_parameter_type
dynamic main(Map<String, dynamic> context) async {
  var sys = context['capabilities']['sys'];
  var llm = context['capabilities']['llm'];

  sys.print("🤖 Intelligent CLI Agent Initialized (Mac-Optimized).");

  var systemPrompt = '''
You are an expert Unix/macOS assistant.
PROTOCOL: Output ONLY valid JSON.
STRATEGY:
1. FINDING APPS: Use `mdfind "App Name"` (broad search) or `ls /Applications`.
2. CHECKING: If 'which' fails, assume it's a GUI app and check /Applications.
3. LOGIC: If you found the info, set "command": null and provide "answer".
4. AVOID LOOPS: Do not repeat failed commands. Try a variation.

JSON FORMAT:
{
  "thought": "reasoning",
  "command": "shell command OR null",
  "safe": true/false,
  "answer": "final answer OR null"
}
''';

  while (true) {
    var input = await sys.readInput("\nuser> ");
    if (input.trim().toLowerCase() == "exit") break;

    var conversation = "SYSTEM: " + systemPrompt + "\nUSER: " + input + "\n";
    
    int steps = 0;
    bool done = false;
    String? lastCmd;

    while (steps < 8 && done == false) {
      var response = await llm.chat(conversation);
      sys.print("DEBUG: raw response=" + response.toString());
      
      try {
        // --- 1. ROBUST PARSING ---
        var clean = response.trim();
        dynamic action;
        try {
          action = jsonDecode(clean);
        } catch (e1) {
          try {
            int start = clean.indexOf('{');
            int end = clean.lastIndexOf('}');
            if (start >= 0 && end > start) {
              var jsonPart = clean.substring(start, end + 1);
              jsonPart = jsonPart.replaceAll(": null", ': ""').replaceAll(":null", ':""');
              action = jsonDecode(jsonPart);
            }
          } catch (e2) {}
        }

        if (action == null || action is! Map) throw "Invalid JSON";

        // --- 2. EXTRACT ---
        var ans = action['answer'];
        var cmd = action['command'];
        var safeVal = action['safe'];
        var thought = action['thought'];

        bool isSafe = false;
        if (safeVal == true || safeVal.toString() == "true") isSafe = true;

        // SAFE CONVERSION
        String cmdStr = "";
        if (cmd != null && cmd.toString() != "null") {
          cmdStr = cmd.toString();
        }

        String ansStr = "";
        if (ans != null && ans.toString() != "null") {
          ansStr = ans.toString();
        }

        sys.print("  💡 " + (thought?.toString() ?? ""));

        // --- 3. LOGIC FLOW ---

        // PRIORITY 0: If we have an answer, PRINT IT and STOP (unless we really need to run a cmd)
        // This prevents the "running empty command" bug.
        if (ansStr.length > 0 && cmdStr.length == 0) {
           sys.print("\n✅ " + ansStr);
           done = true;
        }
        // PRIORITY 1: Run Command
        else if (cmdStr.length > 0) {
          
          // LOOP DETECTION
          if (cmdStr == lastCmd) {
             sys.print("  ⚠️ LOOP DETECTED: Agent tried to run '$cmdStr' again.");
             if (ansStr.length > 0) {
                sys.print("\n✅ (Forced Answer) " + ansStr);
                done = true;
             } else {
                conversation += "\nSYSTEM: ERROR. You just ran '$cmdStr'. It failed. Try a different command.\n";
             }
          } 
          else {
            // Execution
            bool shouldRun = false;
            if (isSafe) {
               sys.print("  🚀 Auto-running: " + cmdStr);
               shouldRun = true;
            } else {
               sys.print("  ⚠️  Requires Confirmation: " + cmdStr);
               shouldRun = await sys.confirm("  Execute? yes/no");
            }

            if (shouldRun) {
              var result = await sys.run('sh', ['-c', cmdStr + " 2>&1"]);
              String out = result == null ? "" : result.toString();
              if (out.trim().length == 0) out = "(No output)";
              
              conversation = conversation + "\nASSISTANT: " + jsonEncode(action) + "\nOBSERVATION: " + out + "\n";
              lastCmd = cmdStr; 
            } else {
               sys.print("  ⛔ User blocked execution.");
               conversation = conversation + "\nASSISTANT: " + jsonEncode(action) + "\nOBSERVATION: User denied permission.\n";
            }
          }
        } 
        else {
           // Case: Both are empty?
           if (ansStr.length > 0) {
              sys.print("\n✅ " + ansStr);
              done = true;
           } else {
              throw "Agent returned neither command nor answer.";
           }
        }

      } catch (e) {
        sys.print("  ⚠️ Error: " + e.toString());
        conversation = conversation + "\nSYSTEM: Invalid format. Return JSON with 'command' OR 'answer'.\n";
      }
      steps = steps + 1;
    }
  }
}