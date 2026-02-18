function usage() {
  print("Usage: hugind agent run agent/coder_setup -- --repo <path> --run-id <id> [--mode build|cleanup] [--worktrees-dir <path>] [--base-ref <ref>] [--branch-prefix <prefix>] [--reuse <true|false>] [--delete-branch <true|false>] [--force <true|false>] [--debug]");
}

function finish(result) {
  const status = String(result && result.status ? result.status : "unknown");
  const summary = String(result && result.summary ? result.summary : "");
  print(`[coder_setup] ${status}${summary ? `: ${summary}` : ""}`);
  const errs = (result && Array.isArray(result.errors)) ? result.errors : [];
  for (let i = 0; i < errs.length; i += 1) {
    print(`[coder_setup] error: ${String(errs[i])}`);
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

function isInside(root, candidate) {
  const r = normalizePath(root);
  const c = normalizePath(candidate);
  if (r === "/") return c.startsWith("/");
  if (c === r) return true;
  return c.startsWith(`${r}/`);
}

function sanitizeRunId(raw) {
  return String(raw || "").replace(/[^A-Za-z0-9._-]/g, "");
}

function toBool(value, fallback) {
  if (value === undefined || value === null || value === "") return fallback;
  const v = String(value).trim().toLowerCase();
  if (v === "true" || v === "1" || v === "yes" || v === "y") return true;
  if (v === "false" || v === "0" || v === "no" || v === "n") return false;
  return fallback;
}

function parseCliArgs(rawArgs) {
  const args = Array.isArray(rawArgs) ? rawArgs.slice() : [];
  if (args[0] === "--") args.shift();

  const options = {
    repo: "",
    runIdRaw: "",
    mode: "build",
    worktreesDir: "",
    baseRef: "",
    branchPrefix: "agent/",
    reuse: true,
    deleteBranch: false,
    force: false,
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

  function getValue(i, token) {
    const value = args[i + 1];
    if (value === undefined || String(value).startsWith("--")) {
      errors.push(`missing value for ${token}`);
      return { ok: false, value: "" };
    }
    return { ok: true, value: String(value) };
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

    if (token === "--repo" || token === "--run-id" || token === "--mode" || token === "--worktrees-dir" || token === "--base-ref" || token === "--branch-prefix") {
      const gv = getValue(i, token);
      if (!gv.ok) {
        i += 1;
        continue;
      }
      if (token === "--repo") setSingle("repo", gv.value, token);
      if (token === "--run-id") setSingle("runIdRaw", gv.value, token);
      if (token === "--mode") setSingle("mode", gv.value, token);
      if (token === "--worktrees-dir") setSingle("worktreesDir", gv.value, token);
      if (token === "--base-ref") setSingle("baseRef", gv.value, token);
      if (token === "--branch-prefix") setSingle("branchPrefix", gv.value, token);
      i += 2;
      continue;
    }

    if (token === "--reuse" || token === "--delete-branch" || token === "--force") {
      const next = args[i + 1];
      let value = "true";
      if (next !== undefined && !String(next).startsWith("--")) {
        value = String(next);
        i += 2;
      } else {
        i += 1;
      }
      if (token === "--reuse") options.reuse = toBool(value, options.reuse);
      if (token === "--delete-branch") options.deleteBranch = toBool(value, options.deleteBranch);
      if (token === "--force") options.force = toBool(value, options.force);
      continue;
    }

    errors.push(`unknown flag: ${token}`);
    i += 1;
  }

  if (!options.repo) errors.push("missing required flag: --repo");
  if (!options.runIdRaw) errors.push("missing required flag: --run-id");
  if (options.mode !== "build" && options.mode !== "cleanup") {
    errors.push("--mode must be build or cleanup");
  }

  return { options, errors };
}

async function runGit(repo, args, audit) {
  const cmd = ["git", "-C", repo].concat(args);
  audit.commands_run.push(cmd.join(" "));
  const out = await spawn("git", ["-C", repo].concat(args));
  return String(out || "").trim();
}

function parseWorktreePorcelain(text) {
  const lines = String(text || "").split("\n");
  const items = [];
  let cur = null;

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    if (!line.trim()) {
      if (cur) items.push(cur);
      cur = null;
      continue;
    }
    if (line.startsWith("worktree ")) {
      if (cur) items.push(cur);
      cur = { path: line.slice(9).trim(), branch: "", head: "" };
      continue;
    }
    if (!cur) continue;
    if (line.startsWith("branch ")) {
      const br = line.slice(7).trim();
      cur.branch = br.startsWith("refs/heads/") ? br.slice("refs/heads/".length) : br;
    } else if (line.startsWith("HEAD ")) {
      cur.head = line.slice(5).trim();
    }
  }
  if (cur) items.push(cur);
  return items;
}

export default async function main(input) {
  const result = {
    status: "failed",
    summary: "",
    repo: "",
    run_id: "",
    mode: "",
    worktree_path: "",
    branch: "",
    base_ref: "",
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
    const repoPath = normalizePath(joinPath(hostCwd, opts.repo));
    if (!fs.exists(repoPath) || !fs.is_dir(repoPath)) {
      result.summary = "Repo path not found";
      result.errors.push(`repo path missing or not dir: ${repoPath}`);
      return finish(result);
    }

    const runId = sanitizeRunId(opts.runIdRaw);
    if (!runId) {
      result.summary = "Invalid run-id";
      result.errors.push("run-id is empty after sanitization (allowed: A-Za-z0-9._-)");
      return finish(result);
    }

    const worktreesDir = opts.worktreesDir
      ? normalizePath(joinPath(hostCwd, opts.worktreesDir))
      : normalizePath(joinPath(repoPath, ".worktrees"));

    if (!isInside(repoPath, worktreesDir)) {
      result.summary = "Invalid worktrees-dir";
      result.errors.push(`worktrees-dir must be inside repo: ${worktreesDir}`);
      return finish(result);
    }

    const branchPrefix = String(opts.branchPrefix || "agent/");
    const branch = `${branchPrefix}${runId}`;
    const worktreePath = normalizePath(joinPath(worktreesDir, runId));

    result.repo = repoPath;
    result.run_id = runId;
    result.mode = opts.mode;
    result.branch = branch;
    result.worktree_path = worktreePath;

    if (opts.debug) {
      print(`[coder_setup] repo=${repoPath}`);
      print(`[coder_setup] mode=${opts.mode}`);
      print(`[coder_setup] run_id=${runId}`);
      print(`[coder_setup] worktree_path=${worktreePath}`);
      print(`[coder_setup] branch=${branch}`);
    }

    await runGit(repoPath, ["rev-parse", "--is-inside-work-tree"], result.audit);

    if (opts.mode === "cleanup") {
      if (fs.exists(worktreePath)) {
        const rmArgs = ["worktree", "remove"];
        if (opts.force) rmArgs.push("--force");
        rmArgs.push(worktreePath);
        await runGit(repoPath, rmArgs, result.audit);
      }

      await runGit(repoPath, ["worktree", "prune"], result.audit);

      if (opts.deleteBranch) {
        const delArgs = ["branch", "-D", branch];
        try {
          await runGit(repoPath, delArgs, result.audit);
        } catch (e) {
          result.errors.push(`branch delete failed: ${String(e)}`);
        }
      }

      result.status = "success";
      result.summary = "Cleanup completed";
      return finish(result);
    }

    if (!fs.exists(worktreesDir)) {
      fs.mkdir(worktreesDir, true);
    }

    const wtText = await runGit(repoPath, ["worktree", "list", "--porcelain"], result.audit);
    const worktrees = parseWorktreePorcelain(wtText);

    const branchHolder = worktrees.find((w) => w.branch === branch && normalizePath(w.path) !== worktreePath);
    if (branchHolder) {
      result.summary = "Branch already checked out";
      result.errors.push(`branch ${branch} is already checked out at ${branchHolder.path}`);
      return finish(result);
    }

    if (fs.exists(worktreePath)) {
      if (!opts.reuse) {
        result.summary = "Worktree already exists";
        result.errors.push(`worktree exists and --reuse=false: ${worktreePath}`);
        return finish(result);
      }

      await runGit(worktreePath, ["rev-parse", "--is-inside-work-tree"], result.audit);
      let currentBranch = "";
      try {
        currentBranch = await runGit(worktreePath, ["rev-parse", "--abbrev-ref", "HEAD"], result.audit);
      } catch (e) {
        currentBranch = "";
      }

      result.base_ref = opts.baseRef || "";
      if (currentBranch && currentBranch !== branch) {
        result.errors.push(`reused worktree on branch ${currentBranch} (expected ${branch})`);
      }

      result.status = "success";
      result.summary = "Reused existing worktree";
      return finish(result);
    }

    const baseRef = opts.baseRef
      ? String(opts.baseRef)
      : await runGit(repoPath, ["rev-parse", "--abbrev-ref", "HEAD"], result.audit);
    result.base_ref = baseRef;

    try {
      await runGit(repoPath, ["worktree", "add", "-b", branch, worktreePath, baseRef], result.audit);
    } catch (e) {
      await runGit(repoPath, ["worktree", "add", worktreePath, branch], result.audit);
    }

    result.status = "success";
    result.summary = "Worktree created";
    return finish(result);
  } catch (e) {
    result.status = "failed";
    result.summary = "Setup failed";
    result.errors.push(String(e));
    return finish(result);
  }
}
