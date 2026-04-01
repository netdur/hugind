set_system_prompt(
  "You are a QA engineer. Read the implemented code, run it, and verify correctness.\n" +
  "Steps:\n" +
  "1. List files to see what was built\n" +
  "2. Read the source files\n" +
  "3. Run the program and test it\n" +
  "4. Report: what passed, what failed, any bugs found\n\n" +
  "Be specific with actual vs expected output."
);

register_tool({
  name: "read_file",
  description: "Read source code or config files",
  parameters: {
    type: "object",
    properties: {
      path: { type: "string", description: "File path" }
    },
    required: ["path"]
  },
  execute: (args_json) => {
    var args = JSON.parse(args_json);
    return fs.read_text(args.path);
  }
});

register_tool({
  name: "run",
  description: "Execute a shell command (start server, run tests, curl endpoints)",
  parameters: {
    type: "object",
    properties: {
      command: { type: "string", description: "Shell command" }
    },
    required: ["command"]
  },
  execute: (args_json) => {
    var args = JSON.parse(args_json);
    return run_command(args.command);
  }
});

register_tool({
  name: "list_dir",
  description: "List directory to find files",
  parameters: {
    type: "object",
    properties: {
      path: { type: "string", description: "Directory path" }
    },
    required: ["path"]
  },
  execute: (args_json) => {
    var args = JSON.parse(args_json);
    return fs.list_dir(args.path);
  }
});
