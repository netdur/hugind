// ignore: main_first_positional_parameter_type
dynamic main(Map<String, dynamic> context) async {
  var sys = context['capabilities']['sys'];
  var llm = context['capabilities']['llm'];
  var args = context['args'] as List;

  // ---------------------------------------------------------
  // 1. GET GIT STATUS
  // ---------------------------------------------------------
  
  var diff = await sys.run("git", ["diff", "--cached"]);
  var isStaged = true;

  if (diff.trim().isEmpty) {
    diff = await sys.run("git", ["diff"]);
    isStaged = false;
  }

  if (diff.trim().isEmpty) {
    sys.print("🤷‍♂️ No changes found.");
    return;
  }

  // ---------------------------------------------------------
  // 2. LLM INTERACTION
  // ---------------------------------------------------------

  if (diff.length > 8000) {
    var sub = diff.substring(0, 8000);
    diff = sub + "\n...[TRUNCATED]";
  }

  var hint = "";
  if (args.isNotEmpty) {
     // Safe join
     var joined = args.join(' ');
     hint = "User hint: $joined";
  }
  
  sys.print("🧠 Analyzing...");
  
  var prompt = '''
You are a git commit message generator.
Task: Analyze the diff and generate a concise conventional commit message.
$hint

REQUIRED OUTPUT FORMAT: JSON ONLY.
Example:
{"message": "feat: add login functionality"}

Diff:
$diff
''';

  var response = await llm.chat(prompt);

  // ---------------------------------------------------------
  // 3. ROBUST PARSING
  // ---------------------------------------------------------
  
  String message = "";
  bool success = false;

  try {
    var clean = response.trim();
    
    // Attempt 1: Direct Decode
    try {
      var data = jsonDecode(clean);
      message = "${data['message']}"; // Interpolate to force String type
      success = true;
    } catch (e1) {
      // Attempt 2: Substring Extraction
      int start = clean.indexOf('{');
      int end = clean.lastIndexOf('}');
      
      if (start >= 0 && end > start) {
        var jsonPart = clean.substring(start, end + 1);
        var data = jsonDecode(jsonPart);
        message = "${data['message']}"; // Interpolate here too
        success = true;
      }
    }
  } catch (e) {
    sys.print("⚠️ Parsing error: $e");
  }

  if (success == false || message.isEmpty) {
    sys.print("⚠️ Could not extract JSON. Raw response:\n$response");
    sys.print("\nPlease enter message manually:");
    message = await sys.readInput("> ");
  }

  sys.print("\n------------------------------------------------");
  sys.print(message);
  sys.print("------------------------------------------------\n");

  // ---------------------------------------------------------
  // 4. WORKFLOW (Stage & Commit)
  // ---------------------------------------------------------
  
  // A. Stage if needed
  if (isStaged == false) {
    sys.print("⚠️ Changes are UNSTAGED.");
    
    var fileList = await sys.run("git", ["diff", "--name-only"]);
    sys.print("\nFiles to add:");
    sys.print(fileList.trim());
    sys.print("");

    var doStage = await sys.confirm("Run 'git add -u' on these files?");
    
    if (doStage) {
      await sys.run("git", ["add", "-u"]);
      sys.print("✅ Files staged.");
    } else {
      sys.print("❌ Must stage files to commit. Exiting.");
      return;
    }
  }

  // B. Commit
  var choice = await sys.readInput("[C]ommit / [E]dit / [Q]uit > ");
  choice = choice.trim().toLowerCase();

  // FIX: We use "$message" inside the list to prevent the Sandbox Crash.
  // The quotes force the runtime to box the value correctly.
  
  if (choice == "c" || choice == "") {
    await sys.run("git", ["commit", "-m", "$message"]);
    sys.print("🚀 Committed!");
  } 
  else if (choice == "e") {
    sys.print("Enter message:");
    var manualMsg = await sys.readInput("> ");
    if (manualMsg.isNotEmpty) {
      await sys.run("git", ["commit", "-m", "$manualMsg"]);
      sys.print("🚀 Committed!");
    }
  } 
  else {
    sys.print("❌ Aborted.");
  }
}