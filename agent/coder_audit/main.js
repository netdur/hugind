function usage() {
  print("Usage: hugind agent run agent/coder_audit -- --task <path> --issue <path> --diff <path> --project <path> [--context <path>] [--cwd <path>] [--debug]");
}

function finish(result) {
  const status = String(result && result.status ? result.status : "unknown");
  const summary = String(result && result.summary ? result.summary : "");
  print(`[coder_audit] ${status}${summary ? `: ${summary}` : ""}`);
  const errs = (result && Array.isArray(result.errors)) ? result.errors : [];
  for (let i = 0; i < errs.length; i += 1) {
    print(`[coder_audit] error: ${String(errs[i])}`);
  }
  set_result(result);
  return result;
}

function toInt(value, fallback) {
  const n = Number(value);
  if (!Number.isFinite(n)) return fallback;
  return Math.trunc(n);
}

function unique(arr) {
  const out = [];
  const seen = {};
  for (let i = 0; i < arr.length; i += 1) {
    const v = String(arr[i]);
    if (!seen[v]) {
      seen[v] = true;
      out.push(v);
    }
  }
  return out;
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

function basename(path) {
  const norm = normalizePath(path);
  if (norm === "/" || norm === ".") return norm;
  const idx = norm.lastIndexOf("/");
  if (idx < 0) return norm;
  return norm.slice(idx + 1);
}

function isInside(root, candidate) {
  const r = normalizePath(root);
  const c = normalizePath(candidate);
  if (r === "/") return c.startsWith("/");
  if (c === r) return true;
  return c.startsWith(`${r}/`);
}

function toRepoRelative(root, path) {
  const r = normalizePath(root);
  const p = normalizePath(path);
  if (p === r) return ".";
  if (p.startsWith(`${r}/`)) return p.slice(r.length + 1);
  return p;
}

function parseCliArgs(rawArgs) {
  const args = Array.isArray(rawArgs) ? rawArgs.slice() : [];
  if (args[0] === "--") args.shift();

  const options = {
    task: "",
    issue: "",
    diff: "",
    project: ".",
    context: "",
    cwd: "",
    maxIters: 3,
    maxFiles: 20,
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

    if (token === "--task" || token === "--issue" || token === "--diff" || token === "--project" || token === "--context" || token === "--cwd") {
      const value = args[i + 1];
      if (value === undefined || String(value).startsWith("--")) {
        errors.push(`missing value for ${token}`);
        i += 1;
        continue;
      }
      if (token === "--task") setSingle("task", value, token);
      if (token === "--issue") setSingle("issue", value, token);
      if (token === "--diff") setSingle("diff", value, token);
      if (token === "--project") setSingle("project", value, token);
      if (token === "--context") setSingle("context", value, token);
      if (token === "--cwd") setSingle("cwd", value, token);
      i += 2;
      continue;
    }

    if (token === "--max-iters" || token === "--max-files") {
      const value = args[i + 1];
      if (value === undefined || String(value).startsWith("--")) {
        errors.push(`missing value for ${token}`);
        i += 1;
        continue;
      }
      if (token === "--max-iters") options.maxIters = toInt(value, options.maxIters);
      if (token === "--max-files") options.maxFiles = toInt(value, options.maxFiles);
      i += 2;
      continue;
    }

    errors.push(`unknown flag: ${token}`);
    i += 1;
  }

  if (!options.task) errors.push("missing required flag: --task");
  if (!options.issue) errors.push("missing required flag: --issue");
  if (!options.diff) errors.push("missing required flag: --diff");

  if (options.maxIters < 1) options.maxIters = 1;
  if (options.maxFiles < 1) options.maxFiles = 1;

  return { options, errors };
}

function parseJsonObject(text) {
  if (text && typeof text === "object") return text;
  const raw = String(text || "").trim();
  try {
    return JSON.parse(raw);
  } catch (_) {
    const fenced = raw.match(/```(?:json)?\s*([\s\S]*?)```/i);
    if (fenced && fenced[1]) return JSON.parse(fenced[1].trim());
  }
  throw new Error("response is not valid JSON object");
}

async function llmJson(prompt, maxFixups) {
  let raw = await llm.chat(prompt);
  const firstRaw = String(raw || "");
  try {
    return { raw: firstRaw, data: parseJsonObject(raw), usedFixup: false, firstRaw, fixedRaw: "", firstParseError: "" };
  } catch (firstErr) {
    if (maxFixups <= 0) throw firstErr;
    const fixPrompt = [
      "Your previous response was invalid JSON.",
      "Return ONLY a valid JSON object with the required schema.",
      "No markdown fences. No explanations.",
      "",
      "Original prompt:",
      prompt,
      "",
      "Previous invalid response:",
      firstRaw
    ].join("\n");
    raw = await llm.chat(fixPrompt);
    const fixedRaw = String(raw || "");
    return {
      raw: fixedRaw,
      data: parseJsonObject(raw),
      usedFixup: true,
      firstRaw,
      fixedRaw,
      firstParseError: String(firstErr)
    };
  }
}

function listDirNames(path) {
  const raw = fs.list_dir(path);
  try {
    const parsed = JSON.parse(String(raw || "[]"));
    if (!Array.isArray(parsed)) return [];
    return parsed.map((v) => String(v)).filter(Boolean);
  } catch (_) {
    return [];
  }
}

function buildProjectTreeProfile(rootPath, maxDepth, maxEntries) {
  const depthLimit = Math.max(1, toInt(maxDepth, 4));
  const entryLimit = Math.max(20, toInt(maxEntries, 300));
  const lines = [];
  let emitted = 0;
  let truncated = false;

  function pushLine(line) {
    if (emitted >= entryLimit) {
      truncated = true;
      return false;
    }
    lines.push(line);
    emitted += 1;
    return true;
  }

  function walk(absPath, relPath, depth) {
    if (depth > depthLimit) return;
    if (!fs.exists(absPath) || !fs.is_dir(absPath)) return;

    const names = listDirNames(absPath).sort();
    for (let i = 0; i < names.length; i += 1) {
      if (truncated) return;
      const name = names[i];
      const childAbs = joinPath(absPath, name);
      const childRel = relPath ? `${relPath}/${name}` : name;
      const isDir = fs.is_dir(childAbs);
      const marker = isDir ? "/" : "";
      if (!pushLine(childRel + marker)) return;
      if (isDir) walk(childAbs, childRel, depth + 1);
    }
  }

  pushLine(".");
  walk(rootPath, "", 1);

  if (truncated) lines.push(`... (truncated at ${entryLimit} entries)`);
  return lines.join("\n");
}

function resolveProjectPath(projectRoot, cwd, rawPath) {
  const raw = String(rawPath || "").trim();
  if (!raw) return { ok: false, error: "path is empty" };
  if (raw.startsWith("/")) return { ok: isInside(projectRoot, raw), absPath: normalizePath(raw) };

  const projectName = basename(projectRoot);
  const candidates = [];
  if (projectName && projectName !== "." && projectName !== "/" && raw.startsWith(`${projectName}/`)) {
    candidates.push(joinPath(projectRoot, raw.slice(projectName.length + 1)));
  }
  candidates.push(joinPath(projectRoot, raw));
  candidates.push(joinPath(cwd, raw));

  const uniq = unique(candidates.map((p) => normalizePath(p)));
  for (let i = 0; i < uniq.length; i += 1) {
    if (isInside(projectRoot, uniq[i])) return { ok: true, absPath: uniq[i] };
  }
  return { ok: false, error: `outside project: ${raw}` };
}

function buildAuditPrompt(params) {
  const taskText = params.taskText;
  const currentIssueText = params.currentIssueText;
  const diffText = params.diffText;
  const knownFiles = params.knownFiles;
  const projectRootRel = params.projectRootRel;
  const treeProfile = params.treeProfile;
  const history = params.history;
  const priorErrors = params.priorErrors;
  const iteration = params.iteration;
  const maxTurns = params.maxTurns;

  const fileBlocks = knownFiles.map((f) => [
    `FILE: ${f.relPath}`,
    "```",
    f.content,
    "```"
  ].join("\n")).join("\n\n");

  const historyBlock = history.length ? history.map((h, idx) => `${idx + 1}. ${h}`).join("\n") : "(none)";
  const errorBlock = priorErrors.length ? priorErrors.join("\n") : "(none)";

  return [
    "You are a patch auditor.",
    `Iteration: ${iteration}/${maxTurns}`,
    "Goal: verify whether the provided patch satisfies the task requirements.",
    "",
    "Return ONLY a JSON object with one action:",
    "1) request_context",
    "2) final_verdict",
    "",
    "Strict schema:",
    "{",
    "  \"action\": \"request_context\" | \"final_verdict\",",
    "  \"reason\": string,",
    "  \"needed_paths\": string[],",
    "  \"status\": \"pass\" | \"fail\",",
    "  \"issues_markdown\": string",
    "}",
    "",
    "Rules:",
    "- For request_context: provide needed_paths; status can be fail and issues_markdown can be empty.",
    "- For final_verdict: set status to pass or fail.",
    "- If status=pass, issues_markdown should be empty or a short pass note.",
    "- If status=fail, issues_markdown must contain concrete actionable issues in markdown.",
    "- Do not include extra keys.",
    `- All needed_paths are relative to project root: ${projectRootRel}.`,
    "- RESPONSE must be raw JSON object only (no fences).",
    "",
    "Task markdown:",
    "```md",
    taskText,
    "```",
    "",
    "Current issue.md content:",
    "```md",
    currentIssueText || "(empty)",
    "```",
    "",
    "Patch under audit (unified diff):",
    "```diff",
    diffText || "(empty)",
    "```",
    "",
    iteration === 1 ? "Project tree profile:" : "",
    iteration === 1 ? "```" : "",
    iteration === 1 ? treeProfile : "",
    iteration === 1 ? "```" : "",
    "",
    "Interaction history:",
    historyBlock,
    "",
    "Previous issues/errors:",
    errorBlock,
    "",
    "Loaded project files:",
    fileBlocks || "(none)",
    "",
    "Now return JSON only."
  ].join("\n");
}

function parseContextPaths(rawText) {
  let doc;
  try {
    doc = JSON.parse(String(rawText || ""));
  } catch (e) {
    throw new Error(`invalid context JSON: ${String(e)}`);
  }

  const ordered = [];
  const seen = {};
  function pushPath(rawPath) {
    const p = String(rawPath || "").trim();
    if (!p || seen[p]) return;
    seen[p] = true;
    ordered.push(p);
  }

  const targetFiles = Array.isArray(doc && doc.target_files) ? doc.target_files : [];
  const supportingFiles = Array.isArray(doc && doc.supporting_files) ? doc.supporting_files : [];

  for (let i = 0; i < targetFiles.length; i += 1) {
    pushPath((targetFiles[i] || {}).path);
  }
  for (let i = 0; i < supportingFiles.length; i += 1) {
    pushPath((supportingFiles[i] || {}).path);
  }

  return ordered;
}

export default async function main(input) {
  const result = {
    status: "failed",
    summary: "",
    verdict: "fail",
    issue_path: "",
    issues_written: false,
    iterations: 0,
    files_read: [],
    files_written: [],
    errors: []
  };

  function noteRead(path) {
    result.files_read.push(path);
    result.files_read = unique(result.files_read);
  }

  function noteWrite(path) {
    result.files_written.push(path);
    result.files_written = unique(result.files_written);
  }

  try {
    const argv = (input && Array.isArray(input.args)) ? input.args : [];
    const parsed = parseCliArgs(argv);
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
    const issuePath = joinPath(cwd, opts.issue);
    const diffPath = joinPath(cwd, opts.diff);
    const projectRoot = joinPath(cwd, opts.project || ".");
    const contextPath = opts.context ? joinPath(cwd, opts.context) : "";

    result.issue_path = issuePath;

    if (opts.debug) {
      print(`[coder_audit] host_cwd=${hostCwd}`);
      print(`[coder_audit] cwd=${cwd}`);
      print(`[coder_audit] task=${taskPath}`);
      print(`[coder_audit] issue=${issuePath}`);
      print(`[coder_audit] diff=${diffPath}`);
      print(`[coder_audit] project=${projectRoot}`);
      if (contextPath) print(`[coder_audit] context=${contextPath}`);
    }

    if (!fs.exists(cwd) || !fs.is_dir(cwd)) {
      result.summary = "CWD path not found";
      result.errors.push(`cwd path missing or not dir: ${cwd}`);
      return finish(result);
    }
    if (!fs.exists(projectRoot) || !fs.is_dir(projectRoot)) {
      result.summary = "Project path not found";
      result.errors.push(`project path missing or not dir: ${projectRoot}`);
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
    if (contextPath && (!fs.exists(contextPath) || !fs.is_file(contextPath))) {
      result.summary = "Context file not found";
      result.errors.push(`context file missing: ${contextPath}`);
      return finish(result);
    }

    const issueDir = dirname(issuePath);
    if (!fs.exists(issueDir)) fs.mkdir(issueDir, true);

    noteRead(taskPath);
    noteRead(diffPath);
    const taskText = fs.read_text(taskPath);
    const diffText = fs.read_text(diffPath);

    let currentIssueText = "";
    if (fs.exists(issuePath) && fs.is_file(issuePath)) {
      noteRead(issuePath);
      currentIssueText = fs.read_text(issuePath);
    }

    const treeProfile = buildProjectTreeProfile(projectRoot, 4, 300);
    if (opts.debug) print("[coder_audit] project tree profile captured");

    const knownFileMap = {};
    const knownFiles = [];
    const history = [];
    const priorErrors = [];

    if (contextPath) {
      noteRead(contextPath);
      let contextPaths;
      try {
        contextPaths = parseContextPaths(fs.read_text(contextPath));
      } catch (e) {
        result.summary = "Invalid context file";
        result.errors.push(String(e));
        return finish(result);
      }

      let seeded = 0;
      for (let i = 0; i < contextPaths.length; i += 1) {
        if (knownFiles.length >= opts.maxFiles) break;
        const rawPath = contextPaths[i];
        const resolved = resolveProjectPath(projectRoot, cwd, rawPath);
        if (!resolved.ok) {
          priorErrors.push(`context path rejected: ${rawPath}`);
          continue;
        }
        const absPath = resolved.absPath;
        if (!fs.exists(absPath) || !fs.is_file(absPath)) {
          priorErrors.push(`context path missing or not file: ${rawPath}`);
          continue;
        }
        const relPath = toRepoRelative(cwd, absPath);
        if (knownFileMap[relPath]) continue;

        const content = fs.read_text(absPath);
        noteRead(absPath);
        knownFiles.push({ relPath, content });
        knownFileMap[relPath] = true;
        seeded += 1;
      }
      history.push(`seed_context_loaded=${seeded}`);
      if (opts.debug) print(`[coder_audit] seeded context files=${seeded}`);
    }

    const maxTurns = Math.max(2, opts.maxIters * 2);
    let turn = 0;

    let finalVerdict = "fail";
    let finalIssues = "";

    while (turn < maxTurns) {
      turn += 1;
      result.iterations = turn;
      const prompt = buildAuditPrompt({
        taskText,
        currentIssueText,
        diffText,
        knownFiles,
        projectRootRel: toRepoRelative(cwd, projectRoot),
        treeProfile,
        history: history.slice(-20),
        priorErrors: priorErrors.slice(-10),
        iteration: turn,
        maxTurns
      });

      if (opts.debug) {
        print(`[coder_audit] llm iteration ${turn}/${maxTurns}`);
        print("[coder_audit] ---- prompt begin ----");
        print(prompt);
        print("[coder_audit] ---- prompt end ----");
      }

      let reply;
      try {
        const llmRes = await llmJson(prompt, 1);
        if (opts.debug) {
          if (llmRes.usedFixup) {
            print(`[coder_audit] first response parse error: ${llmRes.firstParseError}`);
            print("[coder_audit] ---- first (invalid) model response begin ----");
            print(llmRes.firstRaw);
            print("[coder_audit] ---- first (invalid) model response end ----");
            print("[coder_audit] ---- fixup model response begin ----");
            print(llmRes.fixedRaw);
            print("[coder_audit] ---- fixup model response end ----");
          } else {
            print("[coder_audit] ---- model response begin ----");
            print(llmRes.firstRaw);
            print("[coder_audit] ---- model response end ----");
          }
        }
        reply = llmRes.data;
      } catch (e) {
        priorErrors.push(`llm parse failure: ${String(e)}`);
        continue;
      }

      const action = String(reply.action || "");
      const reason = String(reply.reason || "");

      if (action === "request_context") {
        const neededPaths = Array.isArray(reply.needed_paths) ? reply.needed_paths : [];
        history.push(`turn ${turn}: request_context (${neededPaths.length}) reason=${reason || "(none)"}`);

        if (!neededPaths.length) {
          priorErrors.push("request_context with empty needed_paths");
          continue;
        }

        let loaded = 0;
        for (let i = 0; i < neededPaths.length; i += 1) {
          const rawPath = String(neededPaths[i] || "").trim();
          if (!rawPath) continue;

          const resolved = resolveProjectPath(projectRoot, cwd, rawPath);
          if (!resolved.ok) {
            priorErrors.push(`context path rejected: ${rawPath}`);
            continue;
          }

          const absPath = resolved.absPath;
          if (!fs.exists(absPath) || !fs.is_file(absPath)) {
            priorErrors.push(`context path missing or not file: ${rawPath}`);
            continue;
          }

          const relPath = toRepoRelative(cwd, absPath);
          if (knownFileMap[relPath]) continue;
          if (knownFiles.length >= opts.maxFiles) {
            priorErrors.push(`max_files limit reached (${opts.maxFiles})`);
            break;
          }

          const content = fs.read_text(absPath);
          noteRead(absPath);
          knownFiles.push({ relPath, content });
          knownFileMap[relPath] = true;
          loaded += 1;
        }

        history.push(`turn ${turn}: context_loaded=${loaded}`);
        if (loaded === 0) priorErrors.push("no context loaded");
        continue;
      }

      if (action === "final_verdict") {
        const status = String(reply.status || "").toLowerCase();
        const issuesMarkdown = String(reply.issues_markdown || "");
        if (status !== "pass" && status !== "fail") {
          priorErrors.push(`final_verdict with invalid status: ${status}`);
          continue;
        }

        finalVerdict = status;
        finalIssues = issuesMarkdown;
        history.push(`turn ${turn}: final_verdict=${status}`);
        break;
      }

      priorErrors.push(`unknown action: ${action}`);
    }

    if (finalVerdict === "fail") {
      const issueBody = finalIssues && finalIssues.trim()
        ? finalIssues
        : [
          "# Audit Issues",
          "",
          "Patch audit failed, but model did not return structured issue details.",
          "Please review task vs patch manually."
        ].join("\n");
      fs.write_text(issuePath, issueBody);
      noteWrite(issuePath);
      result.issues_written = true;
      result.verdict = "fail";
      result.status = "success";
      result.summary = "Audit failed; issue.md generated";
      return finish(result);
    }

    fs.write_text(issuePath, "");
    noteWrite(issuePath);
    result.issues_written = false;
    result.verdict = "pass";
    result.status = "success";
    result.summary = "Audit passed";
    return finish(result);
  } catch (e) {
    result.status = "failed";
    result.summary = "Unhandled error";
    result.errors.push(String(e));
    return finish(result);
  }
}
