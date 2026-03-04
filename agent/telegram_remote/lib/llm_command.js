export function parseLlmCommand(text) {
  const raw = String(text ?? "").trim();
  if (!raw || !raw.startsWith("/llm")) {
    return null;
  }
  const payload = raw.replace(/^\/llm(?:@\S+)?\s*/i, "");
  return payload.trim();
}

export function buildLlmPrompt(userText) {
  return [
    "You are a Telegram assistant.",
    "Return ONLY valid JSON object with exactly one key:",
    "{\"message\":\"...\"}",
    "No markdown. No extra keys. No prose outside JSON.",
    `User message: ${userText}`,
  ].join("\n");
}

export function extractMessageFromLlmJson(raw) {
  const text = String(raw ?? "").trim();
  if (!text) {
    return "Empty LLM response.";
  }
  try {
    const parsed = JSON.parse(text);
    const message = parsed?.message;
    if (message === undefined || message === null) {
      return "LLM JSON did not include message.";
    }
    const normalized = String(message).trim();
    return normalized || "LLM JSON message was empty.";
  } catch (e) {
    return `Failed to parse LLM JSON: ${String(e)}`;
  }
}

export async function buildReplyForMessage(text) {
  if (text === "/ping") {
    return "pong";
  }
  const llmInput = parseLlmCommand(text);
  if (!llmInput) {
    return 'Use "/llm <message>"';
  }
  const prompt = buildLlmPrompt(llmInput);
  const llmRaw = await llm.chat(prompt);
  return extractMessageFromLlmJson(llmRaw);
}
