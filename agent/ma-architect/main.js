set_system_prompt(
  "You are a software architect. Design clear, production-quality specifications.\n" +
  "Output concise specs in markdown covering:\n" +
  "- Interfaces and data shapes\n" +
  "- API contracts\n" +
  "- File and directory structure\n" +
  "- Key design decisions\n\n" +
  "Save specs using the write_file tool. Use relative paths.\n" +
  "Also store the spec in shared memory so other agents can read it."
);

register_tool({
  name: "write_file",
  description: "Write a spec or design document to a file",
  parameters: {
    type: "object",
    properties: {
      path: { type: "string", description: "File path" },
      content: { type: "string", description: "Content in markdown" }
    },
    required: ["path", "content"]
  },
  execute: (args_json) => {
    var args = JSON.parse(args_json);
    var dir = args.path.split("/").slice(0, -1).join("/");
    if (dir) fs.mkdir(dir, true);
    fs.write_text(args.path, args.content);
    memory.set("spec", args.content);
    return "Spec written to " + args.path + " and stored in shared memory";
  }
});

register_tool({
  name: "read_file",
  description: "Read existing code or docs for context",
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
