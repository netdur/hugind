// ignore: main_first_positional_parameter_type
dynamic main(Map<String, dynamic> context) async {
  var sys = context['capabilities']['sys'];
  var llm = context['capabilities']['llm'];

  sys.print("CLI agent initialized.");

  while (true) {
    var input =
        await sys.readInput("Enter a Unix CLI request (or 'exit' to quit): ");
    var trimmed = input.trim();
    var lower = trimmed.toLowerCase();
    if (lower == "exit" || lower == "quit") {
      sys.print("Goodbye!");
      break;
    }

    if (trimmed.length == 0) {
      sys.print("Please enter a request.");
    } else {
      var prompt =
          "Translate this plain English request into a single safe POSIX shell command. "
          "Return ONLY the command, no explanations, comments, or code fences: " +
              trimmed;
      var command = await llm.chat(prompt);
      command = command.trim();

      if (command.length == 0) {
        sys.print("Could not generate a command. Please try again.");
      } else {
        sys.print("Proposed command: " + command);
        var confirmed = await sys.confirm("Execute this command? yes/no");
        if (confirmed == false) {
          sys.print("Command not executed. Adjust your request and try again.");
        } else {
          try {
            var result = await sys.run('sh', ['-c', command + " 2>&1"]);
            if (result == null || result.trim().isEmpty) {
              sys.print("Command produced no output.");
            } else {
              sys.print(result);
            }
          } catch (e) {
            sys.print("Error executing command: " + e.toString());
          }
          sys.print(command);
        }
      }
    }
  }
}
