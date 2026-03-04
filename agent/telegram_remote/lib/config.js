export function requireToken(input) {
  const token =
    input?.meta?.env?.TELEGRAM_BOT_TOKEN ??
    input?.token ??
    input?.args?.[0] ??
    "";
  if (!token) {
    throw new Error("Missing TELEGRAM_BOT_TOKEN in host env.");
  }
  return String(token).trim();
}

export function getAdminUserId(input) {
  return String(input?.meta?.env?.ADMIN_USER_ID ?? "").trim();
}

export function getLoopSettings() {
  return {
    timeoutSeconds: 30,
    maxUpdatesPerPoll: 50,
  };
}
