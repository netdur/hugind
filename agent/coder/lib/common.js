export function usage() {
  print("Usage: hugind agent run agent/coder -- --task <path> [--issue <path>] --output <path> [--project <path>] [--cwd <path>]");
}

export function finish(result) {
  const status = String(result && result.status ? result.status : "unknown");
  const summary = String(result && result.summary ? result.summary : "");
  print(`[coder] ${status}${summary ? `: ${summary}` : ""}`);
  const errs = (result && Array.isArray(result.errors)) ? result.errors : [];
  for (let i = 0; i < errs.length; i += 1) {
    print(`[coder] error: ${String(errs[i])}`);
  }
  set_result(result);
  return result;
}

export function toInt(value, fallback) {
  const n = Number(value);
  if (!Number.isFinite(n)) return fallback;
  return Math.trunc(n);
}

export function splitLines(text) {
  if (!text) return [];
  return String(text).split("\n");
}

export function unique(arr) {
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

export function safeReadText(path) {
  return fs.read_text(path);
}
