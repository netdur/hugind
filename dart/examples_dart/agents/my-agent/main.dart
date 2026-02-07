dynamic main(Map<String, dynamic> context) async {
  var sys = context['capabilities']['sys'];
  var llm = context['capabilities']['llm'];
  var net = context['capabilities']['net'];
  
  sys.print("Agent initialized.");
  
  while (true) {
    var userInput = await sys.readInput("Enter a Unix CLI request (or 'exit' to quit): ");
    if (userInput.trim().toLowerCase() == "exit") {
      break;
    }
    
    // Ask LLM to translate to shell command
    var prompt = "Translate the following plain English request into a single safe POSIX shell command. Only return the command, no explanations, no wrappers, no code fences. Assume read-only commands unless explicitly requested for destructive actions. Request: $userInput";
    var command = await llm.chat(prompt);
    
    // Show command and ask for confirmation
    sys.print("Proposed command: $command");
    var confirmed = await sys.confirm("Execute this command? yes/no");
    
    if (confirmed == false) {
      sys.print("Command not executed. Please adjust your request or ask for clarification.");
      continue;
    }
    
    // Execute the command
    var result = await sys.run('sh', ['-c', command + " 2>&1"]);
    if (result == null || result.trim().isEmpty) {
      sys.print("Command produced no output.");
    } else {
      sys.print(result);
    }
    
    // Output only the final command (without wrappers, comments, etc.)
    sys.print(command);
  }
}