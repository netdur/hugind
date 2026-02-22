const DEFAULT_LLM_MAX_TOKENS = 8192;

export function parseJsonObject(text) {
  if (text && typeof text === "object") return text;
  const raw = String(text || "").trim();
  try {
    return JSON.parse(raw);
  } catch (_) {
    const fenced = raw.match(/```(?:json)?\s*([\s\S]*?)```/i);
    if (fenced && fenced[1]) {
      return JSON.parse(fenced[1].trim());
    }
  }
  throw new Error("response is not valid JSON object");
}

function normalizeMaxTokens(maxTokens) {
  const n = Number(maxTokens);
  if (!Number.isFinite(n) || n < 1) return DEFAULT_LLM_MAX_TOKENS;
  return Math.trunc(n);
}

async function chatJsonPrompt(prompt, maxTokens) {
  return llm.chat({
    prompt,
    max_tokens: normalizeMaxTokens(maxTokens)
  });
}

export async function llmJson(prompt, maxFixups, maxTokens) {
  let raw = await chatJsonPrompt(prompt, maxTokens);
  const firstRaw = String(raw || "");
  try {
    return {
      raw: firstRaw,
      data: parseJsonObject(raw),
      firstRaw,
      firstParseError: "",
      fixedRaw: "",
      usedFixup: false
    };
  } catch (firstErr) {
    if (maxFixups <= 0) throw firstErr;

    const fixPrompt = [
      "Your previous response was invalid.",
      "Return ONLY a valid JSON object. No markdown. No explanations.",
      "Use the required schema exactly.",
      "",
      "Original prompt:",
      prompt,
      "",
      "Previous invalid response:",
      firstRaw
    ].join("\n");

    raw = await chatJsonPrompt(fixPrompt, maxTokens);
    const fixedRaw = String(raw || "");
    try {
      return {
        raw: fixedRaw,
        data: parseJsonObject(raw),
        firstRaw,
        firstParseError: String(firstErr),
        fixedRaw,
        usedFixup: true
      };
    } catch (secondErr) {
      throw new Error(`response is not valid JSON after fixup; first_error=${String(firstErr)} second_error=${String(secondErr)} first_raw=${firstRaw} second_raw=${fixedRaw}`);
    }
  }
}
