// Discover project root (two levels up from agent/ma-researcher/)
var agent_cwd = fs.cwd();
var project_root = agent_cwd.replace(/\/agent\/[^/]+\/?$/, "");

// Helper: print a thinking line to stderr for real-time progress
function think(msg) {
  eprint("  \u2192 " + msg);
}

// ── Pre-gather project context before the LLM even starts ──────────────────

think("Scanning project structure...");

// Build a tree of the project (2 levels deep, skip noise)
var SKIP = ["target", "node_modules", ".git", "dist", "build", ".DS_Store"];

function tree(dir, depth) {
  if (depth > 2) return "";
  var lines = "";
  try {
    var entries = JSON.parse(fs.list_dir(dir));
    for (var i = 0; i < entries.length; i++) {
      var name = entries[i];
      if (SKIP.indexOf(name) >= 0) continue;
      var full = dir + "/" + name;
      var indent = "";
      for (var d = 0; d < depth; d++) indent += "  ";
      var isDir = false;
      try { isDir = fs.is_dir(full); } catch(e) {}
      if (isDir) {
        lines += indent + name + "/\n";
        lines += tree(full, depth + 1);
      } else {
        lines += indent + name + "\n";
      }
    }
  } catch(e) {}
  return lines;
}

var project_tree = tree(project_root, 0);
think("Project has " + project_tree.split("\n").length + " entries");

// Note: run_command is async and cannot be called at module top-level.
// OS detection is done inside tool execute callbacks instead.

set_system_prompt(
  "You are a research agent. Your job is to answer questions by gathering evidence using tools.\n" +
  "You can investigate codebases, the local filesystem, installed software, running processes, or anything accessible via shell commands.\n\n" +
  "## ENVIRONMENT\n" +
  "- Working directory: " + project_root + "\n" +
  "- Always use ABSOLUTE paths starting with / when reading files.\n\n" +
  "## PROJECT STRUCTURE (if relevant)\n```\n" + project_tree + "```\n\n" +
  "## RULES\n" +
  "- ALWAYS use tools to find evidence. NEVER guess or give generic advice.\n" +
  "- If the question is about the local system, use the run tool to check.\n" +
  "- If the question is about code, use search to find relevant lines FIRST, then read_file only for small files or specific sections.\n" +
  "- Do NOT dump entire large files. Use search to narrow down, then read only what matters.\n" +
  "- Do NOT use 'cat' via run — use read_file instead (it has a 200-line limit for safety).\n" +
  "- Every claim MUST be backed by actual tool output.\n" +
  "- If a tool errors, try a different approach. Do NOT retry the same failing command.\n" +
  "- Stay focused on the question. Do NOT summarize unrelated code.\n" +
  "- Be efficient: run the right command first, don't over-explore.\n\n" +
  "## OUTPUT FORMAT\n" +
  "## Question\nRestate the question.\n\n" +
  "## Evidence\n- What you found, citing tool output.\n\n" +
  "## Answer\nDirect, concise answer based on evidence."
);

// ── Tools ──────────────────────────────────────────────────────────────────

register_tool({
  name: "run",
  description: "Run a shell command and return its output. Use this for system checks, file listings, process inspection, etc.",
  parameters: {
    type: "object",
    properties: {
      command: { type: "string", description: "Shell command to execute" }
    },
    required: ["command"]
  },
  execute: async function(args_json) {
    var args = JSON.parse(args_json);
    if (!args.command) return "Error: missing 'command' argument";
    think("Running: " + args.command);
    try {
      var result = await run_command(args.command);
      var lines = result.split("\n").length;
      think("  done (" + lines + " lines)");
      return result || "(no output)";
    } catch(e) {
      think("  error: " + e.message);
      return "Error: " + e.message;
    }
  }
});

register_tool({
  name: "read_file",
  description: "Read a file. For large files, use grep argument to filter for relevant lines instead of reading everything.",
  parameters: {
    type: "object",
    properties: {
      path: { type: "string", description: "Absolute file path" },
      grep: { type: "string", description: "Optional: only return lines matching this pattern (and 3 lines of context around each match). Recommended for large files." }
    },
    required: ["path"]
  },
  execute: async function(args_json) {
    var args = JSON.parse(args_json);
    if (!args.path) return "Error: missing 'path' argument";

    // If grep is provided, filter the file instead of reading it all
    if (args.grep) {
      think("Reading " + args.path + " (grep: " + args.grep + ")");
      try {
        var result = await run_command("grep -n -C 3 '" + args.grep + "' " + args.path + " 2>/dev/null | head -100");
        var matches = result.split("\n").filter(function(l) { return l.length > 0; }).length;
        think("  " + matches + " matching lines");
        return result || "No lines matching '" + args.grep + "' in " + args.path;
      } catch(e) {
        return "Error: " + e.message;
      }
    }

    think("Reading " + args.path);
    try {
      var content = fs.read_text(args.path);
      var lines = content.split("\n").length;
      think("  " + lines + " lines");
      if (lines > 200) {
        think("  (large file — truncated)");
        var allLines = content.split("\n");
        var head = allLines.slice(0, 50).join("\n");
        var tail = allLines.slice(-20).join("\n");
        content = head + "\n\n... [" + (lines - 70) + " lines omitted — use read_file with grep argument to find specific content] ...\n\n" + tail;
      }
      return content;
    } catch(e) {
      think("  error: " + e.message);
      return "Error: " + e.message;
    }
  }
});

register_tool({
  name: "search",
  description: "Search for a text pattern in files (grep -rn, max 50 results). Good for finding code references.",
  parameters: {
    type: "object",
    properties: {
      pattern: { type: "string", description: "Search pattern (regex supported)" },
      path: { type: "string", description: "Absolute directory or file path to search in" }
    },
    required: ["pattern", "path"]
  },
  execute: async function(args_json) {
    var args = JSON.parse(args_json);
    if (!args.pattern || !args.path) return "Error: missing 'pattern' and/or 'path' arguments";
    think("Searching '" + args.pattern + "' in " + args.path);
    var result = await run_command("grep -rn --include='*.rs' --include='*.js' --include='*.ts' --include='*.yaml' --include='*.toml' --include='*.json' --include='*.md' '" + args.pattern + "' " + args.path + " 2>/dev/null | head -50");
    var matches = result.split("\n").filter(function(l) { return l.length > 0; }).length;
    think("  " + matches + " matches");
    return result || "No matches found.";
  }
});

register_tool({
  name: "list_dir",
  description: "List contents of a directory.",
  parameters: {
    type: "object",
    properties: {
      path: { type: "string", description: "Absolute directory path" }
    },
    required: ["path"]
  },
  execute: function(args_json) {
    var args = JSON.parse(args_json);
    if (!args.path) return "Error: missing 'path' argument";
    think("Listing " + args.path);
    try {
      return fs.list_dir(args.path);
    } catch(e) {
      return "Error: " + e.message;
    }
  }
});

register_tool({
  name: "store_findings",
  description: "Store research findings in shared memory for other agents to read.",
  parameters: {
    type: "object",
    properties: {
      key: { type: "string", description: "Key name for the findings" },
      content: { type: "string", description: "The research findings in markdown" }
    },
    required: ["key", "content"]
  },
  execute: function(args_json) {
    var args = JSON.parse(args_json);
    think("Storing findings under '" + args.key + "'");
    memory.set(args.key, args.content);
    return "Findings stored in shared memory under key: " + args.key;
  }
});

think("Ready with 5 tools");
