set_system_prompt(
  "You are a file writer agent. Use the tools to create directories and write files. " +
  "Write clean, complete file contents."
);

register_tool({
  name: "write_file",
  description: "Write content to a file, creating parent directories if needed",
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
  name: "create_dir",
  description: "Create a directory recursively",
  parameters: {
    type: "object",
    properties: {
      path: { type: "string", description: "Directory path" }
    },
    required: ["path"]
  },
  execute: (args_json) => {
    var args = JSON.parse(args_json);
    fs.mkdir(args.path, true);
    return "Created directory " + args.path;
  }
});

register_tool({
  name: "read_file",
  description: "Read a file to check contents before writing",
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
