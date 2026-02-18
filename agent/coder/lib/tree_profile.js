import { toInt } from "./common.js";
import { joinPath } from "./path_utils.js";

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

export function buildProjectTreeProfile(rootPath, maxDepth, maxEntries) {
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

  if (truncated) {
    lines.push(`... (truncated at ${entryLimit} entries)`);
  }

  return lines.join("\n");
}
