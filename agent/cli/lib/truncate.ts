// @ts-nocheck

export function smartTruncate(text: string, maxChars: i32): string {
  if (text.length <= maxChars) return text;

  const lines = text.split("\n");
  if (lines.length < 20) {
    return "... (truncated)\n" + text.substring(text.length - maxChars);
  }

  const header = lines.slice(0, 5).join("\n");
  let remaining = maxChars - header.length - 50;
  if (remaining <= 0) return text.substring(text.length - maxChars);

  let tailStr = text.substring(text.length - remaining);
  const nextNl = tailStr.indexOf("\n");
  if (nextNl >= 0) tailStr = tailStr.substring(nextNl + 1);

  return header + "\n\n... (middle content truncated) ...\n\n" + tailStr;
}
