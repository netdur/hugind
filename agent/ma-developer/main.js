var spec = memory.get("ma-architect/spec");
var specContext = (spec && spec !== "null")
  ? "\n\nHere is the design spec from the architect:\n" + spec
  : "";

set_system_prompt(
  "You are a developer. Read the spec and implement it.\n" +
  "Write clean, runnable code with proper error handling.\n" +
  "Use the tools to create directories, write files, and test your code.\n" +
  "Use paths relative to the current working directory.\n" +
  "After writing code, run a quick sanity check if possible." +
  specContext
);

register_tool({
  name: "write_file",
  description: "Write content to a file (creates parent directories automatically)",
  parameters: {
    type: "object",
    properties: {
      path: { type: "string", description: "Relative file path" },
      content: { type: "string", description: "File content" }
    },
    required: ["path", "content"]
  },
  execute: (args_json) => {
    var args = JSON.parse(args_json);
    fs.mkdir(args.path.split("/").slice(0, -1).join("/") || ".", true);
    fs.write_text(args.path, args.content);
    return "Written " + args.content.length + " bytes to " + args.path;
  }
});

register_tool({
  name: "read_file",
  description: "Read a file's contents",
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
  description: "Run a shell command and return output",
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
