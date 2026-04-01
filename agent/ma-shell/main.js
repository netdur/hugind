set_system_prompt(
  "You are a shell command agent. Use the run tool to execute commands. " +
  "Report output clearly. If a command fails, include the error output."
);

register_tool({
  name: "run",
  description: "Execute a shell command and return stdout + stderr",
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
  name: "read_file",
  description: "Read a file",
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
