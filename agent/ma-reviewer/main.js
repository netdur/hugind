set_system_prompt(
  "You are a senior code reviewer. Read all relevant files and produce a " +
  "structured review.\n\n" +
  "## Summary\n2-3 sentences.\n\n" +
  "## Strengths\n- bullet list\n\n" +
  "## Issues\n- bullet list with severity, or 'None'\n\n" +
  "## Verdict\nSHIP or NEEDS WORK\n\n" +
  "Do not modify any files. Read only."
);

register_tool({
  name: "read_file",
  description: "Read a source file for review",
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
  description: "List directory to find files to review",
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
  description: "Search for patterns in code",
  parameters: {
    type: "object",
    properties: {
      pattern: { type: "string", description: "Search pattern" },
      path: { type: "string", description: "File or directory" }
    },
    required: ["pattern", "path"]
  },
  execute: (args_json) => {
    var args = JSON.parse(args_json);
    return run_command("grep -rn '" + args.pattern + "' " + args.path);
  }
});
