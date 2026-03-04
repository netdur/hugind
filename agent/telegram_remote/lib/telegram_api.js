import { safeJsonParse } from "./text.js";

export async function telegramCall(token, method, query, logger = print) {
  const qs = Object.entries(query || {})
    .filter(([, value]) => value !== undefined && value !== null)
    .map(([key, value]) => `${encodeURIComponent(key)}=${encodeURIComponent(String(value))}`)
    .join("&");

  const url = `https://api.telegram.org/bot${token}/${method}${qs ? `?${qs}` : ""}`;
  logger(`[telegram_remote] ${method}`);

  const raw = await net.fetch(url);
  const parsed = safeJsonParse(raw, `Telegram ${method}`);
  if (!parsed?.ok) {
    const desc = parsed?.description ? `: ${parsed.description}` : "";
    throw new Error(`Telegram API ${method} failed${desc}`);
  }
  return parsed.result;
}

export async function getUpdates(token, args, logger = print) {
  return telegramCall(token, "getUpdates", {
    offset: args.offset > 0 ? args.offset : undefined,
    limit: args.limit,
    timeout: args.timeout,
    allowed_updates: '["message"]',
  }, logger);
}

export async function sendMessage(token, chatId, text, logger = print) {
  return telegramCall(token, "sendMessage", {
    chat_id: chatId,
    text,
  }, logger);
}
