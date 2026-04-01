set_system_prompt(
  "You are a full-stack developer. Given a task, design and implement it.\n" +
  "Use relative paths. Write clean, runnable code. No placeholders."
);

register_tool({
  name: "write_file",
  description: "Write code or config to a file",
  parameters: {
    type: "object",
    properties: {
      path: { type: "string", description: "File path" },
      content: { type: "string", description: "File content" }
    },
    required: ["path", "content"]
  },
  execute: (args_json) => {
    var args = JSON.parse(args_json);
    var dir = args.path.split("/").slice(0, -1).join("/");
    if (dir) fs.mkdir(dir, true);
    fs.write_text(args.path, args.content);
    return "Written " + args.content.length + " bytes to " + args.path;
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

register_tool({
  name: "list_dir",
  description: "List directory contents",
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

register_tool({
  name: "run",
  description: "Run a shell command",
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
