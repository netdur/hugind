export function nowMs() {
  return Date.now();
}

export function sleep(ms) {
  const end = Date.now() + Math.max(0, Number(ms) || 0);
  while (Date.now() < end) {
    // rquickjs runtime has no setTimeout.
  }
  return Promise.resolve();
}

export function writeJson(path, value) {
  fs.write_text(path, JSON.stringify(value, null, 2));
}

export function randId() {
  return Math.random().toString(36).slice(2, 8);
}

export function clampText(text, maxChars) {
  const s = String(text || "");
  if (s.length <= maxChars) return s;
  return `${s.slice(0, maxChars)}\n...[truncated ${s.length - maxChars} chars]`;
}

