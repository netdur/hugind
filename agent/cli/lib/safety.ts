// @ts-nocheck

export function isIncompleteCommand(cmd: string): bool {
  const c = cmd.trim();
  if (c.length == 0) return true;

  if (c.endsWith("|") || c.endsWith("&&") || c.endsWith("||")) return true;

  if (c.indexOf("find ") == 0 && c.indexOf("-name") >= 0) {
    const parts = c.split(" ");
    if (parts.length > 0 && parts[parts.length - 1] == "-name") return true;
    if (c.indexOf('-name ""') >= 0 || c.indexOf("-name ''") >= 0) return true;
  }

  return false;
}

export function looksUnsafe(cmd: string): bool {
  const c = cmd.trim().toLowerCase();

  if (c.indexOf("rm ") >= 0) return true;
  if (c.indexOf("sudo") >= 0) return true;
  if (c.indexOf("dd ") >= 0) return true;
  if (c.indexOf("mkfs") >= 0) return true;
  if (c.indexOf("find /") >= 0) return true;

  return false;
}

export function shouldExit(inputText: string): bool {
  const v = inputText.trim().toLowerCase();
  return v == "exit" || v == "quit";
}
