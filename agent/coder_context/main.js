function usage() {
  print("Usage: hugind agent run agent/coder_context -- --task <path> --project <path> --context <path> [--cwd <path>] [--max-files <n>] [--max-scan-files <n>] [--max-file-bytes <n>] [--debug]");
}

function finish(result) {
  const status = String(result && result.status ? result.status : "unknown");
  const summary = String(result && result.summary ? result.summary : "");
  print(`[coder_context] ${status}${summary ? `: ${summary}` : ""}`);
  const errs = (result && Array.isArray(result.errors)) ? result.errors : [];
  for (let i = 0; i < errs.length; i += 1) {
    print(`[coder_context] error: ${String(errs[i])}`);
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
    project: ".",
    context: "",
    cwd: "",
    maxFiles: 8,
    maxScanFiles: 800,
    maxFileBytes: 200000,
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

    if (token === "--task" || token === "--project" || token === "--context" || token === "--cwd") {
      const value = args[i + 1];
      if (value === undefined || String(value).startsWith("--")) {
        errors.push(`missing value for ${token}`);
        i += 1;
        continue;
      }
      if (token === "--task") setSingle("task", value, token);
      if (token === "--project") setSingle("project", value, token);
      if (token === "--context") setSingle("context", value, token);
      if (token === "--cwd") setSingle("cwd", value, token);
      i += 2;
      continue;
    }

    if (token === "--max-files" || token === "--max-scan-files" || token === "--max-file-bytes") {
      const value = args[i + 1];
      if (value === undefined || String(value).startsWith("--")) {
        errors.push(`missing value for ${token}`);
        i += 1;
        continue;
      }
      if (token === "--max-files") options.maxFiles = toInt(value, options.maxFiles);
      if (token === "--max-scan-files") options.maxScanFiles = toInt(value, options.maxScanFiles);
      if (token === "--max-file-bytes") options.maxFileBytes = toInt(value, options.maxFileBytes);
      i += 2;
      continue;
    }

    errors.push(`unknown flag: ${token}`);
    i += 1;
  }

  if (!options.task) errors.push("missing required flag: --task");
  if (!options.context) errors.push("missing required flag: --context");
  if (options.maxFiles < 1) options.maxFiles = 1;
  if (options.maxScanFiles < 20) options.maxScanFiles = 20;
  if (options.maxFileBytes < 2048) options.maxFileBytes = 2048;

  return { options, errors };
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

function parseTaskSignals(taskText) {
  const lines = String(taskText || "").split("\n");
  const explicitTargets = [];
  const allowedPaths = [];
  const backtickMatches = [];
  const pathHints = [];
  const symbolHints = [];
  const keywordHints = [];

  const backtickRe = /`([^`]+)`/g;
  let m;
  while ((m = backtickRe.exec(taskText)) !== null) {
    const token = String(m[1] || "").trim();
    if (token) backtickMatches.push(token);
  }

  for (let i = 0; i < lines.length; i += 1) {
    const line = String(lines[i] || "");
    const trimmed = line.trim();
    const targetMatch = trimmed.match(/^target\s+(module|file|path|files|modules)\s*:\s*(.+)$/i);
    if (targetMatch && targetMatch[2]) {
      const raw = targetMatch[2];
      const inline = [];
      let mm;
      while ((mm = backtickRe.exec(raw)) !== null) {
        inline.push(mm[1]);
      }
      if (inline.length > 0) {
        for (let j = 0; j < inline.length; j += 1) explicitTargets.push(String(inline[j]));
      } else {
        const parts = String(raw).split(/,| and | or /i);
        for (let j = 0; j < parts.length; j += 1) {
          const p = parts[j].replace(/^[\s"'`]+|[\s"'`]+$/g, "").trim();
          if (p) explicitTargets.push(p);
        }
      }
    }

    if (/only modify files inside/i.test(trimmed) || /limit changes to/i.test(trimmed)) {
      const inline = [];
      let mm;
      while ((mm = backtickRe.exec(trimmed)) !== null) {
        inline.push(mm[1]);
      }
      for (let j = 0; j < inline.length; j += 1) allowedPaths.push(String(inline[j]));
    }
  }

  for (let i = 0; i < backtickMatches.length; i += 1) {
    const token = backtickMatches[i];
    if (token.includes("/") || token.includes("\\")) {
      pathHints.push(token.replace(/\\/g, "/"));
      continue;
    }
    if (/^[A-Za-z_][A-Za-z0-9_]{2,}$/.test(token)) {
      symbolHints.push(token);
    }
  }

  const stopWords = {
    a: true, an: true, and: true, are: true, as: true, at: true, be: true, by: true, do: true,
    for: true, from: true, if: true, in: true, into: true, is: true, it: true, of: true,
    on: true, or: true, should: true, so: true, such: true, that: true, the: true, then: true,
    this: true, to: true, use: true, when: true, with: true, without: true, not: true, only: true,
    keep: true, change: true, changes: true, file: true, files: true, module: true, modules: true,
    task: true, objective: true, requirement: true, requirements: true, output: true
  };

  const plain = String(taskText || "").toLowerCase().replace(/[^a-z0-9_]+/g, " ");
  const words = plain.split(/\s+/).filter(Boolean);
  for (let i = 0; i < words.length; i += 1) {
    const w = words[i];
    if (w.length < 3) continue;
    if (stopWords[w]) continue;
    if (w.includes("/")) continue;
    keywordHints.push(w);
  }

  return {
    explicitTargets: unique(explicitTargets),
    allowedPaths: unique(allowedPaths),
    pathHints: unique(pathHints),
    symbolHints: unique(symbolHints),
    keywords: unique(keywordHints).slice(0, 80)
  };
}

function extractMarkdownSection(taskText, headingName) {
  const lines = String(taskText || "").split("\n");
  const target = String(headingName || "").trim().toLowerCase();
  if (!target) return "";

  let start = -1;
  for (let i = 0; i < lines.length; i += 1) {
    const line = String(lines[i] || "").trim();
    const m = line.match(/^##\s+(.+)$/);
    if (!m) continue;
    if (String(m[1] || "").trim().toLowerCase() === target) {
      start = i + 1;
      break;
    }
  }
  if (start < 0) return "";

  const out = [];
  for (let i = start; i < lines.length; i += 1) {
    const line = String(lines[i] || "");
    if (/^##\s+/.test(line.trim())) break;
    out.push(line);
  }
  return out.join("\n").trim();
}

function normalizeToProjectRelative(projectRoot, cwd, rawPath) {
  const raw = String(rawPath || "").trim().replace(/\\/g, "/");
  if (!raw) return "";
  const normRaw = normalizePath(raw);

  if (normRaw.startsWith("/")) {
    if (!isInside(projectRoot, normRaw)) return "";
    return toRepoRelative(projectRoot, normRaw);
  }

  const projectName = basename(projectRoot);
  if (projectName && normRaw.startsWith(`${projectName}/`)) {
    return normalizePath(normRaw.slice(projectName.length + 1));
  }

  const fromProject = joinPath(projectRoot, normRaw);
  if (isInside(projectRoot, fromProject)) {
    return toRepoRelative(projectRoot, fromProject);
  }

  const fromCwd = joinPath(cwd, normRaw);
  if (isInside(projectRoot, fromCwd)) {
    return toRepoRelative(projectRoot, fromCwd);
  }

  return "";
}

function walkProjectFiles(projectRoot, maxScanFiles) {
  const denyDirs = {
    ".git": true,
    "node_modules": true,
    "dist": true,
    "build": true,
    "target": true,
    ".worktrees": true
  };

  const out = [];
  const queue = [projectRoot];
  let scannedDirs = 0;

  while (queue.length > 0 && out.length < maxScanFiles) {
    const dir = queue.shift();
    scannedDirs += 1;
    if (!fs.exists(dir) || !fs.is_dir(dir)) continue;

    const names = listDirNames(dir).sort();
    for (let i = 0; i < names.length; i += 1) {
      const name = names[i];
      if (!name || name === "." || name === "..") continue;
      if (denyDirs[name]) continue;

      const abs = joinPath(dir, name);
      if (fs.is_dir(abs)) {
        queue.push(abs);
        continue;
      }
      if (fs.is_file(abs)) {
        out.push(abs);
        if (out.length >= maxScanFiles) break;
      }
    }
  }

  return { files: out, scannedDirs };
}

function getPathTokens(path) {
  const rel = String(path || "").toLowerCase();
  return rel.split(/[^a-z0-9]+/).filter(Boolean);
}

function scoreFile(file, contentText, signals, explicitTargets, allowedRoots, keywordSet) {
  const rel = String(file.rel || "");
  const relLower = rel.toLowerCase();
  const relTokens = getPathTokens(rel);
  let score = 0;
  const reasons = [];

  for (let i = 0; i < explicitTargets.length; i += 1) {
    const target = explicitTargets[i];
    if (!target) continue;
    if (relLower === target) {
      score += 900;
      reasons.push(`exact target: ${target}`);
      break;
    }
    if (relLower.endsWith(`/${target}`) || relLower.endsWith(target)) {
      score += 520;
      reasons.push(`target suffix match: ${target}`);
      break;
    }
    const targetBase = basename(target);
    if (targetBase !== "." && relLower.endsWith(`/${targetBase}`)) {
      score += 250;
      reasons.push(`target basename match: ${targetBase}`);
      break;
    }
  }

  for (let i = 0; i < allowedRoots.length; i += 1) {
    const root = allowedRoots[i];
    if (!root || root === ".") continue;
    if (relLower === root || relLower.startsWith(`${root}/`)) {
      score += 90;
      reasons.push(`inside allowed scope: ${root}`);
      break;
    }
  }

  for (let i = 0; i < signals.pathHints.length; i += 1) {
    const hint = String(signals.pathHints[i] || "").toLowerCase();
    if (!hint) continue;
    if (relLower.includes(hint)) {
      score += 100;
      reasons.push(`path hint: ${hint}`);
      continue;
    }
    const b = basename(hint);
    if (b && b !== "." && relLower.endsWith(`/${b}`)) {
      score += 70;
      reasons.push(`path basename hint: ${b}`);
    }
  }

  let pathKeywordHits = 0;
  for (let i = 0; i < relTokens.length; i += 1) {
    const token = relTokens[i];
    if (!keywordSet[token]) continue;
    pathKeywordHits += 1;
  }
  if (pathKeywordHits > 0) {
    const bounded = Math.min(pathKeywordHits, 6);
    score += bounded * 14;
    reasons.push(`path keywords: ${bounded}`);
  }

  if (contentText) {
    const contentLower = contentText.toLowerCase();

    let symbolHits = 0;
    for (let i = 0; i < signals.symbolHints.length; i += 1) {
      const sym = signals.symbolHints[i];
      if (sym.length < 3) continue;
      if (contentText.includes(sym)) symbolHits += 1;
    }
    if (symbolHits > 0) {
      const bounded = Math.min(symbolHits, 4);
      score += bounded * 35;
      reasons.push(`symbol hints in content: ${bounded}`);
    }

    let keywordHits = 0;
    for (let i = 0; i < signals.keywords.length; i += 1) {
      const kw = signals.keywords[i];
      if (kw.length < 3) continue;
      if (contentLower.includes(kw)) keywordHits += 1;
    }
    if (keywordHits > 0) {
      const bounded = Math.min(keywordHits, 12);
      score += bounded * 3;
      reasons.push(`content keyword hits: ${bounded}`);
    }
  }

  return { score, reasons: unique(reasons) };
}

function inferConfidence(top, second, hasExplicitMatch) {
  if (hasExplicitMatch) return "high";
  if (!top || top.score <= 0) return "low";
  if (top.score >= 180 && (!second || second.score < top.score * 0.6)) return "high";
  if (top.score >= 90) return "medium";
  return "low";
}

function trimReasons(reasons, maxCount) {
  return unique(reasons).slice(0, Math.max(1, maxCount || 3));
}

function nowIso() {
  try {
    return new Date().toISOString();
  } catch (_) {
    return "";
  }
}

export default async function main(input) {
  const result = {
    status: "failed",
    summary: "",
    context_path: "",
    confidence: "low",
    target_files: [],
    supporting_files: [],
    files_scanned: 0,
    files_considered: 0,
    errors: [],
    audit: {
      files_read: [],
      files_written: []
    }
  };

  function noteRead(path) {
    result.audit.files_read.push(path);
    result.audit.files_read = unique(result.audit.files_read);
  }

  function noteWrite(path) {
    result.audit.files_written.push(path);
    result.audit.files_written = unique(result.audit.files_written);
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
    const projectRoot = joinPath(cwd, opts.project || ".");
    const taskPath = joinPath(cwd, opts.task);
    const contextPath = joinPath(cwd, opts.context);
    result.context_path = contextPath;

    print(`[coder_context] host_cwd=${hostCwd}`);
    print(`[coder_context] cwd=${cwd}`);
    print(`[coder_context] task=${taskPath}`);
    print(`[coder_context] project=${projectRoot}`);
    print(`[coder_context] context=${contextPath}`);

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

    noteRead(taskPath);
    const taskText = String(fs.read_text(taskPath) || "");
    const signals = parseTaskSignals(taskText);

    const explicitTargets = [];
    for (let i = 0; i < signals.explicitTargets.length; i += 1) {
      const rel = normalizeToProjectRelative(projectRoot, cwd, signals.explicitTargets[i]);
      if (rel && rel !== ".") explicitTargets.push(rel.toLowerCase());
    }

    const allowedRoots = [];
    for (let i = 0; i < signals.allowedPaths.length; i += 1) {
      const rel = normalizeToProjectRelative(projectRoot, cwd, signals.allowedPaths[i]);
      if (rel) allowedRoots.push(rel.toLowerCase().replace(/\/$/, ""));
    }

    const keywordSet = {};
    for (let i = 0; i < signals.keywords.length; i += 1) keywordSet[signals.keywords[i]] = true;

    const walked = walkProjectFiles(projectRoot, opts.maxScanFiles);
    const files = walked.files;
    result.files_scanned = files.length;
    print(`[coder_context] file_scan complete: ${files.length} file(s), ${walked.scannedDirs} dir(s)`);

    const textExt = {
      ".js": true, ".jsx": true, ".ts": true, ".tsx": true, ".mjs": true, ".cjs": true,
      ".json": true, ".md": true, ".txt": true, ".yaml": true, ".yml": true, ".toml": true,
      ".xml": true, ".html": true, ".css": true, ".scss": true, ".less": true, ".sh": true,
      ".py": true, ".rb": true, ".go": true, ".rs": true, ".java": true, ".kt": true,
      ".swift": true, ".c": true, ".cc": true, ".cpp": true, ".h": true, ".hpp": true,
      ".php": true, ".sql": true
    };

    const scored = [];
    const projectRootNorm = normalizePath(projectRoot);
    for (let i = 0; i < files.length; i += 1) {
      const abs = files[i];
      const rel = toRepoRelative(projectRootNorm, abs);
      let contentText = "";
      let readOk = false;

      try {
        const statRaw = fs.stat(abs);
        const stat = JSON.parse(String(statRaw || "{}"));
        const size = Number(stat && stat.size ? stat.size : 0);
        const dot = rel.lastIndexOf(".");
        const ext = dot >= 0 ? rel.slice(dot).toLowerCase() : "";
        if (size > 0 && size <= opts.maxFileBytes && (textExt[ext] || rel.indexOf(".") < 0)) {
          contentText = String(fs.read_text(abs) || "");
          readOk = true;
          noteRead(abs);
        }
      } catch (_) {
        readOk = false;
      }

      const scoreData = scoreFile({ rel }, contentText, signals, explicitTargets, allowedRoots, keywordSet);
      if (scoreData.score <= 0) continue;

      scored.push({
        path: rel,
        score: scoreData.score,
        reasons: trimReasons(scoreData.reasons, 4),
        sampled_content: readOk
      });
    }

    scored.sort((a, b) => {
      if (b.score !== a.score) return b.score - a.score;
      return a.path.localeCompare(b.path);
    });
    result.files_considered = scored.length;

    const minTargetScore = 20;
    const pickedMap = {};
    const picked = [];
    for (let i = 0; i < scored.length; i += 1) {
      const item = scored[i];
      const isExplicit = explicitTargets.indexOf(String(item.path || "").toLowerCase()) >= 0;
      if (!isExplicit && item.score < minTargetScore) continue;
      picked.push(item);
      pickedMap[item.path] = true;
      if (picked.length >= opts.maxFiles) break;
    }
    const supporting = [];
    for (let i = 0; i < scored.length; i += 1) {
      const item = scored[i];
      if (pickedMap[item.path]) continue;
      supporting.push(item);
      if (supporting.length >= Math.min(20, opts.maxFiles * 2)) break;
    }
    let explicitMatch = false;
    for (let i = 0; i < picked.length; i += 1) {
      const p = picked[i].path.toLowerCase();
      for (let j = 0; j < explicitTargets.length; j += 1) {
        if (p === explicitTargets[j] || p.endsWith(`/${explicitTargets[j]}`)) {
          explicitMatch = true;
          break;
        }
      }
      if (explicitMatch) break;
    }

    const top = picked.length > 0 ? picked[0] : null;
    const second = picked.length > 1 ? picked[1] : null;
    const confidence = inferConfidence(top, second, explicitMatch);
    result.confidence = confidence;

    const targetFiles = picked.map((item) => ({
      path: item.path,
      score: item.score,
      reasons: item.reasons
    }));
    const supportingFiles = supporting.map((item) => ({
      path: item.path,
      score: item.score,
      reasons: item.reasons
    }));

    result.target_files = targetFiles;
    result.supporting_files = supportingFiles;

    const contextDoc = {
      schema_version: "coder_context/v1",
      generated_at: nowIso(),
      task: {
        path: toRepoRelative(cwd, taskPath),
        objective: extractMarkdownSection(taskText, "objective"),
        explicit_targets: signals.explicitTargets,
        allowed_paths: signals.allowedPaths,
        path_hints: signals.pathHints,
        symbol_hints: signals.symbolHints,
        keywords: signals.keywords.slice(0, 30)
      },
      project: {
        root: toRepoRelative(cwd, projectRoot),
        scanned_files: files.length,
        scored_files: scored.length
      },
      confidence,
      target_files: targetFiles,
      supporting_files: supportingFiles,
      recommendations: {
        enforce_target_files_only: confidence !== "low",
        require_manual_target_when_low_confidence: confidence === "low"
      }
    };

    const outDir = dirname(contextPath);
    if (!fs.exists(outDir)) fs.mkdir(outDir, true);
    fs.write_text(contextPath, JSON.stringify(contextDoc, null, 2));
    noteWrite(contextPath);

    result.status = "success";
    if (targetFiles.length === 0) {
      result.summary = "No candidate files found; add Target module or stronger hints";
    } else {
      result.summary = `Context built with ${targetFiles.length} target file(s), confidence=${confidence}`;
    }

    if (opts.debug) {
      print(`[coder_context] explicit_targets=${JSON.stringify(signals.explicitTargets)}`);
      print(`[coder_context] allowed_paths=${JSON.stringify(signals.allowedPaths)}`);
      print(`[coder_context] keywords_count=${signals.keywords.length}`);
    }

    return finish(result);
  } catch (e) {
    result.status = "failed";
    result.summary = "Unexpected error";
    result.errors.push(String(e && e.stack ? e.stack : e));
    return finish(result);
  }
}
