function usage() {
  print("Usage: hugind agent run agent/coder_patcher -- --diff <path> [--project <path>] [--cwd <path>] [--dry-run] [--debug]");
}

function finish(result) {
  const status = String(result && result.status ? result.status : "unknown");
  const summary = String(result && result.summary ? result.summary : "");
  print(`[coder_patcher] ${status}${summary ? `: ${summary}` : ""}`);
  const errs = (result && Array.isArray(result.errors)) ? result.errors : [];
  for (let i = 0; i < errs.length; i += 1) {
    print(`[coder_patcher] error: ${String(errs[i])}`);
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

function isInside(root, candidate) {
  const r = normalizePath(root);
  const c = normalizePath(candidate);
  if (r === "/") return c.startsWith("/");
  if (c === r) return true;
  return c.startsWith(`${r}/`);
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

function splitLines(text) {
  if (text === "") return [""];
  return String(text || "").split("\n");
}

function joinLines(lines) {
  return lines.join("\n");
}

function parseCliArgs(rawArgs) {
  const args = Array.isArray(rawArgs) ? rawArgs.slice() : [];
  if (args[0] === "--") args.shift();

  const options = {
    diff: "",
    project: ".",
    cwd: "",
    dryRun: false,
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

    if (token === "--diff" || token === "--project" || token === "--cwd") {
      const value = args[i + 1];
      if (value === undefined || String(value).startsWith("--")) {
        errors.push(`missing value for ${token}`);
        i += 1;
        continue;
      }
      if (token === "--diff") setSingle("diff", value, token);
      if (token === "--project") setSingle("project", value, token);
      if (token === "--cwd") setSingle("cwd", value, token);
      i += 2;
      continue;
    }

    errors.push(`unknown flag: ${token}`);
    i += 1;
  }

  if (!options.diff) errors.push("missing required flag: --diff");
  return { options, errors };
}

function parseUnifiedDiff(text) {
  const lines = String(text || "").split("\n");
  const files = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];
    if (line.startsWith("diff --git ") || line.startsWith("index ")) {
      i += 1;
      continue;
    }
    if (!line.startsWith("--- ")) {
      i += 1;
      continue;
    }

    const oldHeader = line.slice(4).trim();
    i += 1;
    if (i >= lines.length || !lines[i].startsWith("+++ ")) {
      throw new Error(`Malformed diff: missing +++ after ${oldHeader}`);
    }
    const newHeader = lines[i].slice(4).trim();
    i += 1;

    const hunks = [];
    while (i < lines.length && lines[i].startsWith("@@ ")) {
      const header = lines[i];
      const m = /^@@\s+-(\d+),(\d+)\s+\+(\d+),(\d+)\s+@@/.exec(header);
      if (!m) throw new Error(`Malformed hunk header: ${header}`);
      const hunk = {
        oldStart: Number(m[1]),
        oldCount: Number(m[2]),
        newStart: Number(m[3]),
        newCount: Number(m[4]),
        lines: []
      };
      i += 1;

      while (i < lines.length) {
        const hl = lines[i];
        if (
          hl.startsWith("@@ ") ||
          hl.startsWith("--- ") ||
          hl.startsWith("diff --git ") ||
          hl.startsWith("index ")
        ) break;
        if (hl === "") {
          // Allow blank separator lines between hunks/files.
          i += 1;
          continue;
        }
        if (hl === "\\ No newline at end of file") {
          i += 1;
          continue;
        }
        const prefix = hl[0] || "";
        if (prefix !== " " && prefix !== "+" && prefix !== "-") {
          throw new Error(`Malformed hunk line: ${hl}`);
        }
        hunk.lines.push({ type: prefix, text: hl.slice(1) });
        i += 1;
      }

      hunks.push(hunk);
    }

    files.push({ oldHeader, newHeader, hunks });
  }

  if (files.length === 0) {
    throw new Error("No file patches found in diff");
  }

  return files;
}

function stripHeaderPath(header) {
  const h = String(header || "").trim();
  if (h === "/dev/null") return "";
  if (h.startsWith("a/")) return h.slice(2);
  if (h.startsWith("b/")) return h.slice(2);
  return h;
}

function applyHunks(oldText, hunks, fileRel) {
  const oldLines = splitLines(oldText);
  const out = [];
  let oldIdx = 0;

  for (let h = 0; h < hunks.length; h += 1) {
    const hk = hunks[h];
    const target = Math.max(0, hk.oldStart - 1);

    if (target < oldIdx) {
      throw new Error(`Overlapping hunks at hunk ${h + 1}`);
    }

    while (oldIdx < target && oldIdx < oldLines.length) {
      out.push(oldLines[oldIdx]);
      oldIdx += 1;
    }

    for (let k = 0; k < hk.lines.length; k += 1) {
      const op = hk.lines[k];
      if (op.type === " ") {
        if (oldIdx >= oldLines.length || oldLines[oldIdx] !== op.text) {
          const actual = oldIdx < oldLines.length ? oldLines[oldIdx] : "<EOF>";
          throw new Error(
            `Context mismatch file=${fileRel} hunk=${h + 1} old_range=-${hk.oldStart},${hk.oldCount} new_range=+${hk.newStart},${hk.newCount} op_index=${k + 1} line_no=${oldIdx + 1} expected='${op.text}' actual='${actual}'`
          );
        }
        out.push(op.text);
        oldIdx += 1;
      } else if (op.type === "-") {
        if (oldIdx >= oldLines.length || oldLines[oldIdx] !== op.text) {
          const actual = oldIdx < oldLines.length ? oldLines[oldIdx] : "<EOF>";
          throw new Error(
            `Delete mismatch file=${fileRel} hunk=${h + 1} old_range=-${hk.oldStart},${hk.oldCount} new_range=+${hk.newStart},${hk.newCount} op_index=${k + 1} line_no=${oldIdx + 1} expected='${op.text}' actual='${actual}'`
          );
        }
        oldIdx += 1;
      } else if (op.type === "+") {
        out.push(op.text);
      }
    }
  }

  while (oldIdx < oldLines.length) {
    out.push(oldLines[oldIdx]);
    oldIdx += 1;
  }

  return joinLines(out);
}

export default async function main(input) {
  const result = {
    status: "failed",
    summary: "",
    dry_run: false,
    files_applied: [],
    files_deleted: [],
    files_created: [],
    errors: []
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

    result.dry_run = !!opts.dryRun;

    const hostCwd = normalizePath(fs.realpath(fs.cwd()));
    const cwd = opts.cwd ? joinPath(hostCwd, opts.cwd) : hostCwd;
    const diffPath = joinPath(cwd, opts.diff);
    const projectRoot = joinPath(cwd, opts.project || ".");

    if (opts.debug) {
      print(`[coder_patcher] host_cwd=${hostCwd}`);
      print(`[coder_patcher] cwd=${cwd}`);
      print(`[coder_patcher] diff=${diffPath}`);
      print(`[coder_patcher] project=${projectRoot}`);
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

    if (!fs.exists(diffPath) || !fs.is_file(diffPath)) {
      result.summary = "Diff file not found";
      result.errors.push(`diff file missing: ${diffPath}`);
      return finish(result);
    }

    const diffText = fs.read_text(diffPath);
    const filePatches = parseUnifiedDiff(diffText);
    if (opts.debug) print(`[coder_patcher] parsed file patches=${filePatches.length}`);

    for (let i = 0; i < filePatches.length; i += 1) {
      const fp = filePatches[i];
      const oldRel = stripHeaderPath(fp.oldHeader);
      const newRel = stripHeaderPath(fp.newHeader);
      const targetRel = newRel || oldRel;
      if (!targetRel) {
        throw new Error(`Invalid file patch headers: ${fp.oldHeader} / ${fp.newHeader}`);
      }

      const targetAbs = joinPath(projectRoot, targetRel);
      if (!isInside(projectRoot, targetAbs)) {
        throw new Error(`Patch targets path outside project: ${targetRel}`);
      }

      const oldExists = oldRel ? fs.exists(joinPath(projectRoot, oldRel)) && fs.is_file(joinPath(projectRoot, oldRel)) : false;
      const oldPathAbs = oldRel ? joinPath(projectRoot, oldRel) : targetAbs;
      const oldText = oldExists ? fs.read_text(oldPathAbs) : "";
      if (opts.debug) print(`[coder_patcher] applying file=${targetRel} hunks=${fp.hunks.length} old_exists=${oldExists}`);
      const newText = applyHunks(oldText, fp.hunks, targetRel);

      const isDelete = fp.newHeader === "/dev/null";
      const isCreate = fp.oldHeader === "/dev/null";

      if (!opts.dryRun) {
        if (isDelete) {
          if (fs.exists(targetAbs)) fs.remove(targetAbs, false);
        } else {
          const parent = dirname(targetAbs);
          if (!fs.exists(parent)) fs.mkdir(parent, true);
          fs.write_text(targetAbs, newText);
        }
      }

      result.files_applied.push(targetRel);
      if (isDelete) result.files_deleted.push(targetRel);
      if (isCreate) result.files_created.push(targetRel);
    }

    result.files_applied = unique(result.files_applied);
    result.files_deleted = unique(result.files_deleted);
    result.files_created = unique(result.files_created);

    result.status = "success";
    result.summary = opts.dryRun
      ? `Dry run parsed ${result.files_applied.length} file patch(es)`
      : `Applied ${result.files_applied.length} file patch(es)`;
    return finish(result);
  } catch (e) {
    result.status = "failed";
    result.summary = "Patch apply failed";
    result.errors.push(String(e));
    return finish(result);
  }
}
