import { unique } from "./common.js";

export function normalizePath(path) {
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

export function joinPath(base, next) {
  if (String(next || "").startsWith("/")) return normalizePath(next);
  if (!base || base === ".") return normalizePath(next);
  return normalizePath(`${base}/${next}`);
}

export function dirname(path) {
  const norm = normalizePath(path);
  if (norm === "/") return "/";
  const idx = norm.lastIndexOf("/");
  if (idx <= 0) return norm.startsWith("/") ? "/" : ".";
  return norm.slice(0, idx);
}

export function basename(path) {
  const norm = normalizePath(path);
  if (norm === "/" || norm === ".") return norm;
  const idx = norm.lastIndexOf("/");
  if (idx < 0) return norm;
  return norm.slice(idx + 1);
}

export function isInside(root, candidate) {
  const r = normalizePath(root);
  const c = normalizePath(candidate);
  if (r === "/") return c.startsWith("/");
  if (c === r) return true;
  return c.startsWith(`${r}/`);
}

export function toRepoRelative(root, path) {
  const r = normalizePath(root);
  const p = normalizePath(path);
  if (p === r) return ".";
  if (p.startsWith(`${r}/`)) return p.slice(r.length + 1);
  return p;
}

export function resolveModelPath(projectRoot, cwd, rawPath, options) {
  const opts = options || {};
  const requireExistingFile = !!opts.requireExistingFile;
  const raw = String(rawPath || "").trim();
  if (!raw) {
    return { ok: false, error: "path is empty" };
  }

  if (raw.startsWith("/")) {
    return { ok: true, absPath: normalizePath(raw) };
  }

  const projectName = basename(projectRoot);
  const candidates = [];

  if (projectName && projectName !== "." && projectName !== "/" && raw.startsWith(`${projectName}/`)) {
    candidates.push(joinPath(projectRoot, raw.slice(projectName.length + 1)));
  }

  candidates.push(joinPath(projectRoot, raw));
  candidates.push(joinPath(cwd, raw));

  const uniqueCandidates = unique(candidates.map((p) => normalizePath(p)));

  if (requireExistingFile) {
    for (let i = 0; i < uniqueCandidates.length; i += 1) {
      const p = uniqueCandidates[i];
      if (!isInside(projectRoot, p)) continue;
      if (fs.exists(p) && fs.is_file(p)) {
        return { ok: true, absPath: p };
      }
    }
  }

  for (let i = 0; i < uniqueCandidates.length; i += 1) {
    const p = uniqueCandidates[i];
    if (isInside(projectRoot, p)) {
      return { ok: true, absPath: p };
    }
  }

  return { ok: false, error: `path resolves outside project: ${raw}` };
}
