export function safeJsonParse(raw, context) {
  try {
    return JSON.parse(raw);
  } catch (e) {
    throw new Error(`${context}: invalid JSON (${String(e)})`);
  }
}

export function clampTextForLog(value, maxLen = 120) {
  const str = String(value ?? "");
  if (str.length <= maxLen) {
    return str;
  }
  return `${str.slice(0, maxLen)}...`;
}

export function splitTelegramText(text, maxLen = 3500) {
  const source = String(text ?? "");
  if (!source) {
    return [""];
  }
  const parts = [];
  for (let i = 0; i < source.length; i += maxLen) {
    parts.push(source.slice(i, i + maxLen));
  }
  return parts;
}
