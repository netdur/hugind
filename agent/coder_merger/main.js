function usage() {
  print("Usage: hugind agent run agent/coder_merger -- --repo <path> --branch <name> [--worktree <path>] [--task <path>] [--docs <path>] [--diff <path>] [--message <msg>] [--llm-message] [--no-merge] [--dry-run] [--cwd <path>] [--debug]");
}

function finish(result) {
  const status = String(result && result.status ? result.status : "unknown");
  const summary = String(result && result.summary ? result.summary : "");
  print(`[coder_merger] ${status}${summary ? `: ${summary}` : ""}`);
  const errs = (result && Array.isArray(result.errors)) ? result.errors : [];
  for (let i = 0; i < errs.length; i += 1) {
    print(`[coder_merger] error: ${String(errs[i])}`);
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

function parseCliArgs(rawArgs) {
  const args = Array.isArray(rawArgs) ? rawArgs.slice() : [];
  if (args[0] === "--") args.shift();

  const options = {
    repo: "",
    branch: "",
    worktree: "",
    task: "",
    docs: "",
    diff: "",
    message: "",
    llmMessage: false,
    merge: true,
    dryRun: false,
    debug: false,
    cwd: "",
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
    if (token === "--no-merge") {
      options.merge = false;
      i += 1;
      continue;
    }
    if (token === "--llm-message") {
      options.llmMessage = true;
      i += 1;
      continue;
    }
    if (token === "--dry-run") {
      options.dryRun = true;
      i += 1;
      continue;
    }
    if (token === "--debug") {
      options.debug = true;
      i += 1;
      continue;
    }

    if (token === "--repo" || token === "--branch" || token === "--worktree" || token === "--task" || token === "--docs" || token === "--diff" || token === "--message" || token === "--cwd") {
      const value = args[i + 1];
      if (value === undefined || String(value).startsWith("--")) {
        errors.push(`missing value for ${token}`);
        i += 1;
        continue;
      }
      if (token === "--repo") setSingle("repo", value, token);
      if (token === "--branch") setSingle("branch", value, token);
      if (token === "--worktree") setSingle("worktree", value, token);
      if (token === "--task") setSingle("task", value, token);
      if (token === "--docs") setSingle("docs", value, token);
      if (token === "--diff") setSingle("diff", value, token);
      if (token === "--message") setSingle("message", value, token);
      if (token === "--cwd") setSingle("cwd", value, token);
      i += 2;
      continue;
    }

    errors.push(`unknown flag: ${token}`);
    i += 1;
  }

  if (!options.repo) errors.push("missing required flag: --repo");
  if (!options.branch) errors.push("missing required flag: --branch");
  return { options, errors };
}

async function runGit(repo, args, audit) {
  const cmd = ["git", "-C", repo].concat(args);
  const cmdText = cmd.join(" ");
  audit.commands_run.push(cmdText);
  const out = await spawn("git", ["-C", repo].concat(args));
  const text = String(out || "").trim();
  if (text.startsWith("Error:")) {
    throw new Error(`git command failed: ${cmdText}\n${text}`);
  }
  return text;
}

async function resolveBranch(repoPath, requestedBranch, audit) {
  const direct = String(requestedBranch || "").trim();
  if (!direct) throw new Error("branch cannot be empty");

  try {
    await runGit(repoPath, ["show-ref", "--verify", "--quiet", `refs/heads/${direct}`], audit);
    return direct;
  } catch (_) {
    // Fall through to common setup branch prefix.
  }

  const prefixed = `agent/${direct}`;
  try {
    await runGit(repoPath, ["show-ref", "--verify", "--quiet", `refs/heads/${prefixed}`], audit);
    return prefixed;
  } catch (_) {
    throw new Error(`branch not found: ${direct} (also tried ${prefixed})`);
  }
}

function buildCommitPrompt(taskText, docsText, diffText) {
  return [
    "Write a concise git commit message for the following change.",
    "Rules:",
    "- Return strict JSON only.",
    "- Schema: {\"subject\":\"...\",\"body\":\"...\"}.",
    "- subject: <= 72 chars imperative summary.",
    "- body: optional, max 4 short bullet points as plain text.",
    "",
    "Task:",
    "```md",
    taskText || "(none)",
    "```",
    "",
    "Docs:",
    "```md",
    docsText || "(none)",
    "```",
    "",
    "Patch:",
    "```diff",
    diffText || "(none)",
    "```"
  ].join("\n");
}

function sanitizeCommitMessage(text) {
  const raw = String(text || "").trim();
  if (!raw) return "Apply generated patch";
  try {
    const parsed = JSON.parse(raw);
    if (typeof parsed === "string" && parsed.trim()) return parsed.trim();
    if (parsed && typeof parsed === "object") {
      const keys = ["subject", "commit_message", "message", "commit", "response", "content"];
      for (let i = 0; i < keys.length; i += 1) {
        const v = parsed[keys[i]];
        if (typeof v === "string" && v.trim()) return v.trim();
      }
      const vals = Object.values(parsed);
      for (let i = 0; i < vals.length; i += 1) {
        if (typeof vals[i] === "string" && vals[i].trim()) return vals[i].trim();
      }
    }
  } catch (_) {
    // plain text
  }
  return raw;
}

export default async function main(input) {
  const result = {
    status: "failed",
    summary: "",
    repo: "",
    worktree: "",
    branch: "",
    commit_message: "",
    commit_sha: "",
    merged: false,
    dry_run: false,
    errors: [],
    audit: {
      commands_run: []
    }
  };

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
    const effectiveCwd = opts.cwd ? joinPath(hostCwd, opts.cwd) : hostCwd;
    const repoPath = normalizePath(joinPath(effectiveCwd, opts.repo));
    const worktreePath = opts.worktree
      ? normalizePath(joinPath(effectiveCwd, opts.worktree))
      : normalizePath(joinPath(repoPath, ".worktrees", opts.branch.replace(/[\/]/g, "_")));

    const docsPath = opts.docs ? normalizePath(joinPath(effectiveCwd, opts.docs)) : "";
    const diffPath = opts.diff ? normalizePath(joinPath(effectiveCwd, opts.diff)) : "";
    const taskPath = opts.task ? normalizePath(joinPath(effectiveCwd, opts.task)) : normalizePath(joinPath(worktreePath, "task.md"));

    result.repo = repoPath;
    result.worktree = worktreePath;
    result.branch = opts.branch;
    result.dry_run = !!opts.dryRun;

    if (opts.debug) {
      print(`[coder_merger] repo=${repoPath}`);
      print(`[coder_merger] worktree=${worktreePath}`);
      print(`[coder_merger] branch=${opts.branch}`);
    }

    if (!fs.exists(repoPath) || !fs.is_dir(repoPath)) {
      result.summary = "Repo path not found";
      result.errors.push(`repo path missing or not dir: ${repoPath}`);
      return finish(result);
    }

    if (!fs.exists(worktreePath) || !fs.is_dir(worktreePath)) {
      result.summary = "Worktree path not found";
      result.errors.push(`worktree path missing or not dir: ${worktreePath}`);
      return finish(result);
    }

    await runGit(repoPath, ["rev-parse", "--is-inside-work-tree"], result.audit);
    await runGit(worktreePath, ["rev-parse", "--is-inside-work-tree"], result.audit);

    const mergeBranch = await resolveBranch(repoPath, opts.branch, result.audit);
    result.branch = mergeBranch;

    let commitMessage = String(opts.message || "").trim();
    if (!commitMessage && opts.llmMessage) {
      let docsText = "";
      let diffText = "";
      let taskText = "";
      if (taskPath && fs.exists(taskPath) && fs.is_file(taskPath)) taskText = fs.read_text(taskPath);
      if (docsPath && fs.exists(docsPath) && fs.is_file(docsPath)) docsText = fs.read_text(docsPath);
      if (diffPath && fs.exists(diffPath) && fs.is_file(diffPath)) diffText = fs.read_text(diffPath);
      if (!docsText) {
        const fallbackDocs = normalizePath(joinPath(worktreePath, "docs.md"));
        if (fs.exists(fallbackDocs) && fs.is_file(fallbackDocs)) docsText = fs.read_text(fallbackDocs);
      }
      if (!diffText) {
        const fallbackDiff = normalizePath(joinPath(worktreePath, "output.diff"));
        if (fs.exists(fallbackDiff) && fs.is_file(fallbackDiff)) diffText = fs.read_text(fallbackDiff);
      }

      const prompt = buildCommitPrompt(taskText, docsText, diffText);
      if (opts.debug) print("[coder_merger] generating commit message via llm");
      const raw = await llm.chat(prompt);
      commitMessage = sanitizeCommitMessage(raw);
    }
    if (!commitMessage) {
      commitMessage = `Apply patch ${opts.branch}`;
    }
    result.commit_message = commitMessage;
    if (opts.debug) print(`[coder_merger] commit_message=${commitMessage}`);

    if (opts.dryRun) {
      result.status = "success";
      result.summary = "Dry run completed (no git mutations)";
      return finish(result);
    }

    const statusBefore = await runGit(worktreePath, ["status", "--porcelain"], result.audit);
    if (!statusBefore.trim()) {
      result.status = "failed";
      result.summary = "No changes to commit";
      result.errors.push("No changes to commit (did you apply the patch?)");
      return finish(result);
    }

    await runGit(worktreePath, ["add", "-A"], result.audit);
    await runGit(worktreePath, ["commit", "-m", commitMessage], result.audit);
    result.commit_sha = await runGit(worktreePath, ["rev-parse", "HEAD"], result.audit);

    if (opts.merge) {
      await runGit(repoPath, ["merge", "--ff-only", mergeBranch], result.audit);
      result.merged = true;
    }

    result.status = "success";
    result.summary = opts.merge ? "Committed and merged" : "Committed";
    return finish(result);
  } catch (e) {
    result.status = "failed";
    result.summary = "Merge flow failed";
    result.errors.push(String(e));
    return finish(result);
  }
}
