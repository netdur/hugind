export const SYSTEM_PROMPT = `You are a Chrome Tester Agent.

You control a browser through Chrome MCP tools.
Return ONLY one valid JSON object for the next action.

## HOW TO WORK
- The page is represented as an accessibility tree with element UIDs (e.g. uid=1_42).
- BROWSER_STATE shows a compact summary: only interactive elements (links, buttons, inputs…), headings, and landmarks — statictext children are omitted. Use full_snapshot if you need the entire tree.
- navigate, click_uid, fill_uid, press_key all change the page. Always use UIDs from the MOST RECENT snapshot (they change after every action).
- To submit a search box after fill_uid, use press_key with key="Enter".
- If the element you need isn't visible in the snapshot, use scroll to find it.
- If a click opens a new tab (target=_blank links, OAuth popups, etc.), the agent will auto-switch. If it didn't and the page looks unchanged after a click, check PAGES for a new tab and use select_page to switch.
- Follow the task literally. If the task says type "X", type EXACTLY "X" — never translate or substitute.

Allowed actions:
- navigate
- snapshot
- full_snapshot
- list_pages
- select_page
- click_uid
- fill_uid
- type_text
- press_key
- scroll
- wait_for_text
- evaluate
- screenshot
- detect_blocking_overlay
- dismiss_blocking_overlay
- wait
- done

## RULES
1. Use UIDs from SNAPSHOT_STATE for click/fill actions.
2. Use short, reversible steps. Re-read state often.
3. If state is unchanged after repeated steps, change strategy.
3.1 If OVERLAY_CHECK.blocked is true, prefer dismiss_blocking_overlay before continuing.
4. Use done as soon as goal is complete. Include the answer/result in done.reason.
5. Output strict JSON only.
6. Required fields:
   - navigate: url
   - click_uid: uid
   - fill_uid: uid, text
   - type_text: text
   - press_key: key
   - scroll: direction (down|up|top|bottom)
   - wait_for_text: text
   - select_page: index (number)
   - evaluate: script
7. If unsure, use wait or snapshot.

## RESPONSE FORMAT
Always use this exact JSON shape:
{"action": "<action_name>", ...params, "reason": "why"}

Examples:
{"action": "navigate", "url": "https://example.com", "reason": "go to target site"}
{"action": "click_uid", "uid": "1_42", "reason": "click the search button"}
{"action": "fill_uid", "uid": "2_10", "text": "hello", "reason": "type in search box"}
{"action": "snapshot", "reason": "read the current page state"}
{"action": "done", "reason": "task complete"}`;

const ALLOWED_ACTIONS = {
  navigate: true,
  snapshot: true,
  full_snapshot: true,
  list_pages: true,
  select_page: true,
  click_uid: true,
  fill_uid: true,
  type_text: true,
  press_key: true,
  scroll: true,
  wait_for_text: true,
  evaluate: true,
  screenshot: true,
  detect_blocking_overlay: true,
  dismiss_blocking_overlay: true,
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

  // Handle shorthand format like {"navigate": "url"} or {"snapshot": ""}
  if (!decision.action || typeof decision.action !== "string") {
    const keys = Object.keys(decision);
    const actionKey = keys.find((k) => ALLOWED_ACTIONS[k]);
    if (actionKey) {
      const val = decision[actionKey];
      const converted = { action: actionKey, reason: decision.reason || "" };
      if (typeof val === "string" && val) {
        // Map the value to the right param based on action
        if (actionKey === "navigate") converted.url = val;
        else if (actionKey === "click_uid") converted.uid = val;
        else if (actionKey === "type_text") converted.text = val;
        else if (actionKey === "press_key") converted.key = val;
        else if (actionKey === "wait_for_text") converted.text = val;
        else if (actionKey === "evaluate") converted.script = val;
        else if (actionKey === "scroll") converted.direction = val;

      } else if (typeof val === "object" && val) {
        Object.assign(converted, val);
      }
      // Also copy any extra fields (uid, text, etc.) from top level
      for (const k of keys) {
        if (k !== actionKey && k !== "reason" && converted[k] === undefined) {
          converted[k] = decision[k];
        }
      }
      decision = converted;
    }
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
  if (decision.action === "scroll" && decision.direction && !["down", "up", "top", "bottom"].includes(String(decision.direction).toLowerCase())) return "scroll direction must be down|up|top|bottom";
  if (decision.action === "evaluate" && !hasNonEmptyString(decision.script)) return "evaluate requires script";
  return null;
}

function trace(msg) {
  try { eprint("[trace] " + msg); } catch (_e) {}
}

// Send the system prompt once at session start
export async function sendSystemPrompt() {
  trace("Sending system prompt (" + SYSTEM_PROMPT.length + " chars)");
  const raw = await llm.chat({
    messages: [
      { role: "system", content: SYSTEM_PROMPT },
      { role: "user", content: "Ready. Waiting for the first task." },
    ],
    response_format: { type: "json_object" },
    stream: false,
  });
  trace("System prompt ack: " + String(raw || "").slice(0, 200));
}

// Send a plain string prompt — server tracks history via X-Session-ID
export async function getDecision(prompt) {
  print("############## PROMPT ##############");
  print(prompt);
  print("############ / PROMPT ##############");

  const raw = await llm.chat(prompt);

  print("############## RESPONSE ############");
  print(String(raw || ""));
  print("############ / RESPONSE ############");

  let decision = parseJsonResponse(raw);
  let err = validateDecision(decision);
  if (!err) {
    trace("Decision OK: " + JSON.stringify(decision));
    return decision;
  }

  trace("Validation failed: " + err + " — sending repair prompt");

  const repairPrompt =
    `Your previous JSON action was invalid.\n` +
    `Validation error: ${err}\n` +
    `Previous output: ${String(raw).slice(0, 400)}\n\n` +
    `Return ONLY one corrected JSON object.`;

  const repairedRaw = await llm.chat(repairPrompt);
  trace("LLM repair response: " + String(repairedRaw || "").slice(0, 1000));

  decision = parseJsonResponse(repairedRaw);
  err = validateDecision(decision);
  if (!err) {
    trace("Repair OK: " + JSON.stringify(decision));
    return decision;
  }

  trace("Repair also failed: " + err);
  return { action: "wait", reason: `Invalid action JSON after retry: ${err}` };
}
