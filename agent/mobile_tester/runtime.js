export function log(msg) {
  print(String(msg));
}

export function nowMs() {
  return Date.now();
}

export function sleep(ms) {
  const end = Date.now() + Math.max(0, Number(ms) || 0);
  while (Date.now() < end) {
    // rquickjs runtime does not expose setTimeout; use a simple blocking wait.
  }
  return Promise.resolve();
}

export function readJson(path) {
  return JSON.parse(fs.read_text(path));
}

export function writeJson(path, value) {
  fs.write_text(path, JSON.stringify(value, null, 2));
}

const BASE64_TABLE = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

export function bytesToBase64(bytes) {
  let out = "";
  for (let i = 0; i < bytes.length; i += 3) {
    const b0 = bytes[i] | 0;
    const b1 = i + 1 < bytes.length ? bytes[i + 1] : 0;
    const b2 = i + 2 < bytes.length ? bytes[i + 2] : 0;
    const n = (b0 << 16) | (b1 << 8) | b2;
    out += BASE64_TABLE[(n >>> 18) & 63];
    out += BASE64_TABLE[(n >>> 12) & 63];
    out += i + 1 < bytes.length ? BASE64_TABLE[(n >>> 6) & 63] : "=";
    out += i + 2 < bytes.length ? BASE64_TABLE[n & 63] : "=";
  }
  return out;
}

export function randId() {
  return Math.random().toString(36).slice(2, 8);
}
