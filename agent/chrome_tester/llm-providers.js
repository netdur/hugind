export const SYSTEM_PROMPT = `You are a Chrome Tester Agent.

You control a browser through Chrome MCP tools.
Return ONLY one valid JSON object for the next action.

Allowed actions:
- navigate
- snapshot
- list_pages
- select_page
- click_uid
- fill_uid
- type_text
- press_key
- wait_for_text
- evaluate
- screenshot
- detect_blocking_overlay
- dismiss_blocking_overlay
- extract_phone_numbers
- wait
- done

Rules:
1. Use UIDs from SNAPSHOT_STATE for click/fill actions.
2. Prefer: snapshot -> select target uid -> click/fill.
3. Use short, reversible steps. Re-read state often.
4. If state is unchanged after repeated steps, change strategy.
4.1 If OVERLAY_CHECK.blocked is true, prefer dismiss_blocking_overlay before continuing.
4.2 If goal asks for phone number, use extract_phone_numbers and include the selected number in done.reason.
5. Use done as soon as goal is complete.
6. Output strict JSON only.
7. Required fields:
   - navigate: url
   - click_uid: uid
   - fill_uid: uid, text
   - type_text: text
   - press_key: key
   - wait_for_text: text
   - select_page: index (number)
   - evaluate: script
8. If unsure, use wait or snapshot.`;

const ALLOWED_ACTIONS = {
  navigate: true,
  snapshot: true,
  list_pages: true,
  select_page: true,
  click_uid: true,
  fill_uid: true,
  type_text: true,
  press_key: true,
  wait_for_text: true,
  evaluate: true,
  screenshot: true,
  detect_blocking_overlay: true,
  dismiss_blocking_overlay: true,
  extract_phone_numbers: true,
  wait: true,
  done: true,
};

function hasNonEmptyString(v) {
  return typeof v === "string" && v.trim().length > 0;
}

export function trimMessages(messages, maxHistorySteps) {
  if (!messages.length) return messages;
  const system = messages[0].role === "system" ? messages[0] : null;
  const rest = system ? messages.slice(1) : messages;
  const maxMessages = maxHistorySteps * 2;
  if (rest.length <= maxMessages) return messages;
  const dropped = rest.length - maxMessages;
  const summary = { role: "user", content: `[${Math.floor(dropped / 2)} earlier steps omitted]` };
  const trimmed = rest.slice(dropped);
  return system ? [system, summary, ...trimmed] : [summary, ...trimmed];
}

function sanitizeJsonText(raw) {
  return String(raw || "").replace(/\n/g, " ").replace(/\r/g, " ");
}

export function parseJsonResponse(text) {
  let decision = null;
  try {
    decision = JSON.parse(text);
  } catch (_e) {
    try {
      decision = JSON.parse(sanitizeJsonText(text));
    } catch (_e2) {
      const m = String(text || "").match(/\{[\s\S]*\}/);
      if (m) {
        try {
          decision = JSON.parse(sanitizeJsonText(m[0]));
        } catch (_e3) {}
      }
    }
  }

  if (!decision || typeof decision !== "object") {
    return { action: "wait", reason: "Failed to parse model response" };
  }

  if (!decision.action && decision.next_action && typeof decision.next_action === "object") {
    decision = decision.next_action;
  }

  if (!decision.action || typeof decision.action !== "string") {
    return { action: "wait", reason: "Invalid decision format: missing action" };
  }

  decision.action = decision.action.trim().toLowerCase();
  return decision;
}

export function validateDecision(decision) {
  if (!decision || typeof decision !== "object") return "Decision must be an object";
  if (!hasNonEmptyString(decision.action)) return "Missing action";
  if (!ALLOWED_ACTIONS[decision.action]) return `Unknown action: ${decision.action}`;

  if (decision.action === "navigate" && !hasNonEmptyString(decision.url)) return "navigate requires url";
  if (decision.action === "click_uid" && !hasNonEmptyString(decision.uid)) return "click_uid requires uid";
  if (decision.action === "fill_uid") {
    if (!hasNonEmptyString(decision.uid)) return "fill_uid requires uid";
    if (!hasNonEmptyString(decision.text)) return "fill_uid requires text";
  }
  if (decision.action === "type_text" && !hasNonEmptyString(decision.text)) return "type_text requires text";
  if (decision.action === "press_key" && !hasNonEmptyString(decision.key)) return "press_key requires key";
  if (decision.action === "wait_for_text" && !hasNonEmptyString(decision.text)) return "wait_for_text requires text";
  if (decision.action === "select_page" && !Number.isFinite(decision.index)) return "select_page requires numeric index";
  if (decision.action === "evaluate" && !hasNonEmptyString(decision.script)) return "evaluate requires script";
  return null;
}

export async function getDecision(messages) {
  const request = {
    messages,
    response_format: { type: "json_object" },
    stream: false,
  };

  const raw = await llm.chat(request);
  let decision = parseJsonResponse(raw);
  let err = validateDecision(decision);
  if (!err) return decision;

  const repairPrompt =
    `Your previous JSON action was invalid.\n` +
    `Validation error: ${err}\n` +
    `Previous output: ${String(raw).slice(0, 400)}\n\n` +
    `Return ONLY one corrected JSON object.`;

  const repairedRaw = await llm.chat({
    messages: [...messages, { role: "user", content: repairPrompt }],
    response_format: { type: "json_object" },
    stream: false,
  });

  decision = parseJsonResponse(repairedRaw);
  err = validateDecision(decision);
  if (!err) return decision;

  return { action: "wait", reason: `Invalid action JSON after retry: ${err}` };
}
