set_system_prompt(
  "You are a file reader agent. Use the tools to read files, list directories, " +
  "and search for content. Return what you find as structured text."
);

register_tool({
  name: "read_file",
  description: "Read the contents of a file",
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
  description: "List files and directories",
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
  name: "search",
  description: "Search for a pattern in files using grep",
  parameters: {
    type: "object",
    properties: {
      pattern: { type: "string", description: "Search pattern" },
      path: { type: "string", description: "File or directory to search" }
    },
    required: ["pattern", "path"]
  },
  execute: (args_json) => {
    var args = JSON.parse(args_json);
    return run_command("grep -rn '" + args.pattern + "' " + args.path);
  }
});
