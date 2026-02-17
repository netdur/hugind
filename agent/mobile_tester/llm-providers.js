import { sanitizeCoordinates } from "./actions.js";

export const SYSTEM_PROMPT = `You are an Android Driver Agent. Your job is to achieve the user's goal by navigating the Android UI.

Return ONLY a valid JSON object for the next action.

Allowed actions: tap, type, enter, swipe, home, back, wait, done, longpress, screenshot, launch, clear, clipboard_get, clipboard_set, paste, shell, scroll, open_url, switch_app, notifications, pull_file, push_file, keyevent, open_settings, read_screen, submit_message, copy_visible_text, wait_for_content, find_and_tap, compose_email.

Rules:
1. Use coordinates from SCREEN_CONTEXT center values.
2. For type and paste, include coordinates when possible.
3. If an action fails or screen is unchanged, switch strategy.
4. Use done immediately when goal is complete.
5. Output strict JSON only.
6. REQUIRED FIELDS:
   - launch: must include package, or uri, or both package+activity.
   - switch_app: must include package.
   - open_url: must include url.
   - keyevent: must include code (integer).
   - tap/longpress: must include coordinates [x, y].
   - type: must include text (and coordinates when field choice matters).
7. Never output actions with missing required fields. If unsure, use wait.
8. APP LAUNCH RECOVERY:
   - If launch/switch_app fails (package not found, no launcher activity, or repeated launch errors), DO NOT repeat the same package.
   - First run: {"action":"shell","command":"pm list packages","reason":"Discover installed apps"}
   - Then choose an installed package matching the goal and retry launch/switch_app with that package.
   - For SMS, prefer installed candidates like com.google.android.apps.messaging before legacy com.android.mms.
9. Use LAST_ACTION_RESULT. If it reports launch failure, immediately switch to package discovery recovery.`;

export const OUTPUT_EXAMPLES = `
Valid output examples:
{"action":"launch","package":"com.google.android.keep","reason":"Open Google Keep"}
{"action":"tap","coordinates":[540,1800],"reason":"Tap New note"}
{"action":"type","coordinates":[540,600],"text":"buy wine","reason":"Enter note text"}
{"action":"done","reason":"Note is saved and visible"}
{"action":"shell","command":"pm list packages","reason":"Launch failed, discover installed packages"}
{"action":"launch","package":"com.google.android.apps.messaging","reason":"Use installed Messages package after discovery"}
`;

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

export function sanitizeJsonText(raw) {
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
    return { action: "wait", reason: "Failed to parse response, waiting" };
  }

  // Normalize common wrapped formats.
  if (!decision.action && decision.next_action && typeof decision.next_action === "object") {
    decision = decision.next_action;
  } else if (!decision.action && decision.decision && typeof decision.decision === "object") {
    decision = decision.decision;
  } else if (!decision.action && decision.result && typeof decision.result === "object") {
    decision = decision.result;
  }

  // Normalize top-level action-key format, e.g. {"launch": {"package":"..."}}
  if (!decision.action && decision && typeof decision === "object") {
    const keys = Object.keys(decision);
    for (const k of keys) {
      const actionName = String(k).trim().toLowerCase();
      if (ALLOWED_ACTIONS[actionName]) {
        const payload = decision[k];
        if (payload && typeof payload === "object" && !Array.isArray(payload)) {
          decision = { ...payload, action: actionName };
        } else {
          decision = { action: actionName };
        }
        break;
      }
    }
  }

  if (decision && typeof decision.action === "object" && decision.action !== null) {
    const maybeName = decision.action.name || decision.action.type || decision.action.action;
    if (maybeName) {
      decision = { ...decision, ...decision.action, action: maybeName };
    }
  }

  if (!decision || typeof decision.action !== "string" || !decision.action.trim()) {
    return {
      action: "wait",
      reason: `Invalid decision format (missing action). Raw: ${String(text || "").slice(0, 160)}`,
    };
  }

  decision.action = decision.action.trim();
  decision.coordinates = sanitizeCoordinates(decision.coordinates);
  return decision;
}

const ALLOWED_ACTIONS = {
  tap: true,
  type: true,
  enter: true,
  swipe: true,
  home: true,
  back: true,
  wait: true,
  done: true,
  longpress: true,
  screenshot: true,
  launch: true,
  clear: true,
  clipboard_get: true,
  clipboard_set: true,
  paste: true,
  shell: true,
  scroll: true,
  open_url: true,
  switch_app: true,
  notifications: true,
  pull_file: true,
  push_file: true,
  keyevent: true,
  open_settings: true,
  read_screen: true,
  submit_message: true,
  copy_visible_text: true,
  wait_for_content: true,
  find_and_tap: true,
  compose_email: true,
};

function hasCoords(v) {
  return Array.isArray(v) && v.length >= 2 && Number.isFinite(v[0]) && Number.isFinite(v[1]);
}

function hasNonEmptyString(v) {
  return typeof v === "string" && v.trim().length > 0;
}

export function validateDecision(decision) {
  if (!decision || typeof decision !== "object") return "Decision must be an object";
  if (!hasNonEmptyString(decision.action)) return "Missing action";
  const action = decision.action.trim().toLowerCase();
  decision.action = action;

  if (!ALLOWED_ACTIONS[action]) return `Unknown action: ${action}`;

  if ((action === "tap" || action === "longpress") && !hasCoords(decision.coordinates)) {
    return `${action} requires coordinates [x, y]`;
  }
  if (action === "type" && !hasNonEmptyString(decision.text)) {
    return "type requires text";
  }
  if (action === "switch_app" && !hasNonEmptyString(decision.package)) {
    return "switch_app requires package";
  }
  if (action === "open_url" && !hasNonEmptyString(decision.url)) {
    return "open_url requires url";
  }
  if (action === "keyevent" && !Number.isFinite(decision.code)) {
    return "keyevent requires numeric code";
  }
  if (action === "launch") {
    const hasPkg = hasNonEmptyString(decision.package);
    const hasUri = hasNonEmptyString(decision.uri);
    const hasActivity = hasNonEmptyString(decision.activity);
    if (!hasPkg && !hasUri && !hasActivity) {
      return "launch requires package or uri or package+activity";
    }
    if (hasActivity && !hasPkg) {
      return "launch with activity requires package";
    }
  }
  if (action === "find_and_tap" && !hasNonEmptyString(decision.query)) {
    return "find_and_tap requires query";
  }
  if (action === "compose_email" && !hasNonEmptyString(decision.query)) {
    return "compose_email requires query (recipient email)";
  }
  if (action === "pull_file" && !hasNonEmptyString(decision.path)) {
    return "pull_file requires path";
  }
  if (action === "push_file" && (!hasNonEmptyString(decision.source) || !hasNonEmptyString(decision.dest))) {
    return "push_file requires source and dest";
  }
  if (action === "open_settings" && !hasNonEmptyString(decision.setting)) {
    return "open_settings requires setting";
  }

  return null;
}

function toOpenAIMessage(msg) {
  if (typeof msg.content === "string") {
    return { role: msg.role, content: msg.content };
  }
  const parts = [];
  for (const part of msg.content) {
    if (part.type === "text") {
      parts.push({ type: "text", text: part.text });
    } else if (part.type === "image") {
      parts.push({ type: "image_url", image_url: { url: `data:${part.mimeType};base64,${part.base64}`, detail: "low" } });
    }
  }
  return { role: msg.role, content: parts };
}

export async function getDecision(messages, streamingEnabled) {
  const request = {
    messages: messages.map(toOpenAIMessage),
    response_format: { type: "json_object" },
    stream: false,
  };

  let raw = "";
  if (streamingEnabled) {
    let buf = "";
    await llm.chat_stream({
      ...request,
      stream: true,
      on_token: (tok) => {
        buf += tok;
      },
    });
    raw = buf;
  } else {
    raw = await llm.chat(request);
  }

  let decision = parseJsonResponse(raw);
  let err = validateDecision(decision);
  if (!err) return decision;

  // One repair round-trip when model emits malformed or incomplete JSON.
  const repairPrompt =
    `Your previous response was invalid JSON action output.\n` +
    `Validation error: ${err}\n` +
    `Previous output: ${String(raw).slice(0, 400)}\n\n` +
    `${OUTPUT_EXAMPLES}\n` +
    `Return ONLY one corrected JSON object.`;

  const repairedRaw = await llm.chat({
    response_format: { type: "json_object" },
    messages: [...request.messages, { role: "user", content: repairPrompt }],
    stream: false,
  });

  decision = parseJsonResponse(repairedRaw);
  err = validateDecision(decision);
  if (!err) return decision;

  return { action: "wait", reason: `Invalid action JSON after retry: ${err}` };
}
