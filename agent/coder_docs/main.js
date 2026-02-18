function usage() {
  print("Usage: hugind agent run agent/coder_docs -- --task <path> --diff <path> --docs <path> [--issue <path>] [--cwd <path>] [--debug]");
}

function finish(result) {
  const status = String(result && result.status ? result.status : "unknown");
  const summary = String(result && result.summary ? result.summary : "");
  print(`[coder_docs] ${status}${summary ? `: ${summary}` : ""}`);
  const errs = (result && Array.isArray(result.errors)) ? result.errors : [];
  for (let i = 0; i < errs.length; i += 1) {
    print(`[coder_docs] error: ${String(errs[i])}`);
  }
  set_result(result);
  return result;
}

function normalizePath(path) {
  const raw = String(path || "");
  const isAbs = raw.startsWith("/");
  const parts = raw.split("/");
  const out = [];
  for (let i = 0; i < parts.length; i += 1) {
    const part = parts[i];
    if (!part || part === ".") continue;
    if (part === "..") {
      if (out.length > 0) out.pop();
      continue;
    }
    out.push(part);
  }
  const joined = out.join("/");
  if (isAbs) return `/${joined}` || "/";
  return joined || ".";
}

function joinPath(base, next) {
  if (String(next || "").startsWith("/")) return normalizePath(next);
  if (!base || base === ".") return normalizePath(next);
  return normalizePath(`${base}/${next}`);
}

function dirname(path) {
  const norm = normalizePath(path);
  if (norm === "/") return "/";
  const idx = norm.lastIndexOf("/");
  if (idx <= 0) return norm.startsWith("/") ? "/" : ".";
  return norm.slice(0, idx);
}

function parseCliArgs(rawArgs) {
  const args = Array.isArray(rawArgs) ? rawArgs.slice() : [];
  if (args[0] === "--") args.shift();

  const options = {
    task: "",
    issue: "",
    diff: "",
    docs: "",
    cwd: "",
    debug: false,
    help: false
  };

  const errors = [];
  const seen = {};

  function setSingle(key, value, flag) {
    if (seen[key]) {
      errors.push(`duplicate flag: ${flag}`);
      return;
    }
    seen[key] = true;
    options[key] = String(value || "");
  }

  let i = 0;
  while (i < args.length) {
    const token = String(args[i] || "");

    if (token === "--help" || token === "-h") {
      options.help = true;
      i += 1;
      continue;
    }
    if (token === "--debug") {
      options.debug = true;
      i += 1;
      continue;
    }

    if (token === "--task" || token === "--issue" || token === "--diff" || token === "--docs" || token === "--cwd") {
      const value = args[i + 1];
      if (value === undefined || String(value).startsWith("--")) {
        errors.push(`missing value for ${token}`);
        i += 1;
        continue;
      }
      if (token === "--task") setSingle("task", value, token);
      if (token === "--issue") setSingle("issue", value, token);
      if (token === "--diff") setSingle("diff", value, token);
      if (token === "--docs") setSingle("docs", value, token);
      if (token === "--cwd") setSingle("cwd", value, token);
      i += 2;
      continue;
    }

    errors.push(`unknown flag: ${token}`);
    i += 1;
  }

  if (!options.task) errors.push("missing required flag: --task");
  if (!options.diff) errors.push("missing required flag: --diff");
  if (!options.docs) errors.push("missing required flag: --docs");

  return { options, errors };
}

function buildDocsPrompt(taskText, issueText, diffText) {
  return [
    "You are a documentation writer for a generated patch.",
    "Write concise, practical docs in Markdown.",
    "",
    "Required sections (exact headings):",
    "# Summary",
    "# What Changed",
    "# Files Affected",
    "# Tests to run",
    "",
    "Rules:",
    "- Base content only on task/issue/diff input.",
    "- Do not claim tests were executed.",
    "- In 'Tests to run', provide concrete commands if inferable; otherwise write 'Not applicable'.",
    "- Keep it concise and actionable.",
    "- Return Markdown only.",
    "",
    "Task:",
    "```md",
    taskText,
    "```",
    "",
    "Issue (optional):",
    "```md",
    issueText || "(none)",
    "```",
    "",
    "Patch diff:",
    "```diff",
    diffText,
    "```"
  ].join("\n");
}

function extractDocsMarkdown(rawText) {
  const raw = String(rawText || "").trim();
  if (!raw) return "";

  try {
    const parsed = JSON.parse(raw);
    if (typeof parsed === "string") return parsed.trim();
    if (parsed && typeof parsed === "object") {
      const preferredKeys = [
        "docs_markdown",
        "docs",
        "markdown",
        "content",
        "response",
        "message"
      ];
      for (let i = 0; i < preferredKeys.length; i += 1) {
        const key = preferredKeys[i];
        if (typeof parsed[key] === "string" && parsed[key].trim()) {
          return parsed[key].trim();
        }
      }
    }
  } catch (_) {
    // not JSON, treat as plain markdown
  }

  return raw;
}

export default async function main(input) {
  const result = {
    status: "failed",
    summary: "",
    docs_path: "",
    files_read: [],
    files_written: [],
    errors: []
  };

  function noteRead(path) {
    result.files_read.push(path);
  }

  function noteWrite(path) {
    result.files_written.push(path);
  }

  try {
    const args = (input && Array.isArray(input.args)) ? input.args : [];
    const parsed = parseCliArgs(args);
    const opts = parsed.options;

    if (opts.help) {
      usage();
      result.status = "needs_input";
      result.summary = "Help requested";
      result.errors = parsed.errors;
      return finish(result);
    }

    if (parsed.errors.length > 0) {
      usage();
      result.status = "needs_input";
      result.summary = "Invalid CLI arguments";
      result.errors = parsed.errors;
      return finish(result);
    }

    const hostCwd = normalizePath(fs.realpath(fs.cwd()));
    const cwd = opts.cwd ? joinPath(hostCwd, opts.cwd) : hostCwd;
    const taskPath = joinPath(cwd, opts.task);
    const issuePath = opts.issue ? joinPath(cwd, opts.issue) : "";
    const diffPath = joinPath(cwd, opts.diff);
    const docsPath = joinPath(cwd, opts.docs);

    result.docs_path = docsPath;

    if (opts.debug) {
      print(`[coder_docs] host_cwd=${hostCwd}`);
      print(`[coder_docs] cwd=${cwd}`);
      print(`[coder_docs] task=${taskPath}`);
      if (issuePath) print(`[coder_docs] issue=${issuePath}`);
      print(`[coder_docs] diff=${diffPath}`);
      print(`[coder_docs] docs=${docsPath}`);
    }

    if (!fs.exists(cwd) || !fs.is_dir(cwd)) {
      result.summary = "CWD path not found";
      result.errors.push(`cwd path missing or not dir: ${cwd}`);
      return finish(result);
    }

    if (!fs.exists(taskPath) || !fs.is_file(taskPath)) {
      result.summary = "Task file not found";
      result.errors.push(`task file missing: ${taskPath}`);
      return finish(result);
    }

    if (!fs.exists(diffPath) || !fs.is_file(diffPath)) {
      result.summary = "Diff file not found";
      result.errors.push(`diff file missing: ${diffPath}`);
      return finish(result);
    }

    if (issuePath && (!fs.exists(issuePath) || !fs.is_file(issuePath))) {
      result.summary = "Issue file not found";
      result.errors.push(`issue file missing: ${issuePath}`);
      return finish(result);
    }

    noteRead(taskPath);
    noteRead(diffPath);
    const taskText = fs.read_text(taskPath);
    const diffText = fs.read_text(diffPath);

    let issueText = "";
    if (issuePath) {
      noteRead(issuePath);
      issueText = fs.read_text(issuePath);
    }

    const prompt = buildDocsPrompt(taskText, issueText, diffText);
    if (opts.debug) print("[coder_docs] generating docs from task/issue/diff");

    const llmRaw = String(await llm.chat(prompt) || "");
    const docsMarkdown = extractDocsMarkdown(llmRaw);
    if (!docsMarkdown) {
      result.summary = "LLM returned empty docs";
      result.errors.push("empty docs output");
      return finish(result);
    }

    const docsDir = dirname(docsPath);
    if (!fs.exists(docsDir)) fs.mkdir(docsDir, true);
    fs.write_text(docsPath, docsMarkdown + "\n");
    noteWrite(docsPath);

    result.status = "success";
    result.summary = "docs.md generated";
    return finish(result);
  } catch (e) {
    result.status = "failed";
    result.summary = "Unhandled error";
    result.errors.push(String(e));
    return finish(result);
  }
}
