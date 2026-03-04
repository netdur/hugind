const OFFSET_FILE = "telegram_offset.txt";

export function readOffset() {
  if (typeof fs === "undefined") {
    return 0;
  }
  try {
    const raw = fs.read_text(OFFSET_FILE);
    const n = parseInt(String(raw).trim(), 10);
    return Number.isFinite(n) && n >= 0 ? n : 0;
  } catch {
    return 0;
  }
}

export function writeOffset(offset) {
  if (typeof fs === "undefined") {
    return;
  }
  try {
    fs.write_text(OFFSET_FILE, String(offset));
  } catch {
    // Ignore persistence errors in remote bot loop.
  }
}
