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

export async function llmJson(prompt, maxFixups) {
  let raw = await llm.chat(prompt);
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

    raw = await llm.chat(fixPrompt);
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
