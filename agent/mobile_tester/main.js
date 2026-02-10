// --- GLOBAL STATE ---
let UI_CACHE = [];
let DEVICE_ID = null;

// Keep false to avoid extra app-specific heuristics.
// This version removes the overfit "input + query => type" shortcut,
// and replaces it with intent-aware, app-agnostic guardrails.
const USE_HEURISTICS = false;

// --- HELPERS ---
function sleep(ms) {
  // QuickJS: no setTimeout; busy-wait is OK here
  const start = Date.now();
  while (Date.now() - start < ms) {}
}

function normalize(s) {
  return (s || "").toLowerCase();
}

function escapeAdbText(text) {
  // adb shell input text quirks:
  // - spaces => %s
  // - escape common shell-sensitive chars
  return String(text)
    .replace(/\\/g, "\\\\")
    .replace(/ /g, "%s")
    .replace(/&/g, "\\&")
    .replace(/\(/g, "\\(")
    .replace(/\)/g, "\\)")
    .replace(/</g, "\\<")
    .replace(/>/g, "\\>")
    .replace(/;/g, "\\;")
    .replace(/'/g, "\\'")
    .replace(/"/g, '\\"')
    .replace(/\|/g, "\\|")
    .replace(/\$/g, "\\$")
    .replace(/!/g, "\\!")
    .replace(/\n/g, "%n");
}

function historyHasTyped(history, text) {
  if (!history || !text) return false;
  return history.some(
    (h) => h && h.tool === "mobile_type_keys" && h.args && h.args.text === text
  );
}

function lastAction(history) {
  return history && history.length ? history[history.length - 1] : null;
}

function historyJustTapped(history, idx) {
  const last = lastAction(history);
  return (
    last &&
    last.tool === "tap_element" &&
    last.args &&
    typeof last.args.index === "number" &&
    last.args.index === idx
  );
}

function extractGoalKeywords(goal) {
  if (!goal) return [];
  return goal
    .toLowerCase()
    .replace(/[^a-z0-9 ]+/g, " ")
    .split(/\s+/)
    .filter((w) => w.length >= 4);
}

// --- Intent extraction (app-agnostic) ---
function extractSearchQuery(goal) {
  if (!goal) return null;

  // Only treat as search intent if goal contains a search verb
  if (!/\b(search|find|look\s*up|lookup|query)\b/i.test(goal)) return null;

  const m =
    goal.match(/\bsearch\b.*?["'](.+?)["']/i) ||
    goal.match(/search\s+for\s+["']?(.+?)["']?$/i) ||
    goal.match(/search\s*:\s*["']?(.+?)["']?$/i);

  return m && m[1] ? m[1].trim() : null;
}

function extractCreateText(goal) {
  if (!goal) return null;

  // Only treat as create intent if goal contains creation verbs + object nouns
  if (!/\b(add|create|write|make|new)\b/i.test(goal)) return null;
  if (!/\b(note|memo|reminder|task|item|entry|message|post)\b/i.test(goal)) return null;

  const m = goal.match(/["'](.+?)["']/);
  return m && m[1] ? m[1].trim() : null;
}

// --- UI classification (app-agnostic) ---
function elementString(e) {
  if (!e) return "";
  return normalize([e.desc, e.text, e.contentDesc, e.resId, e.klass].join(" "));
}

function firstInputIndex() {
  for (let i = 0; i < UI_CACHE.length; i++) {
    if (UI_CACHE[i] && UI_CACHE[i].isEdit) return i;
  }
  return -1;
}

function countInputs() {
  let n = 0;
  for (let i = 0; i < UI_CACHE.length; i++) {
    if (UI_CACHE[i] && UI_CACHE[i].isEdit) n++;
  }
  return n;
}

function screenLooksSearchy() {
  // Generic indicators that we're in a "search mode":
  // - any element string contains "search"
  // - any element id contains "search"
  // - any element contains "voice" (often paired with search)
  for (let i = 0; i < UI_CACHE.length; i++) {
    const s = elementString(UI_CACHE[i]);
    if (s.includes("search") || s.includes("voice")) return true;
  }
  return false;
}

function findCreateAffordanceIndex() {
  // Generic creation affordances across apps:
  // create/new/add/compose/plus/fab/note
  const needles = ["create", "new", "add", "compose", "plus", "fab", "note", "write"];
  for (let i = 0; i < UI_CACHE.length; i++) {
    const s = elementString(UI_CACHE[i]);
    // Prefer clearly actionable buttons
    if (needles.some((n) => s.includes(n))) return i;
  }
  return -1;
}

function wouldRepeatSameTap(history, nextIndex) {
  const last = lastAction(history);
  return (
    last &&
    last.tool === "tap_element" &&
    last.args &&
    typeof last.args.index === "number" &&
    last.args.index === nextIndex
  );
}

// --- LLM ---
async function ask_llm(goal, history, screenContext, hints) {
  const prompt = [
    "You are a mobile automation planner for an Android device.",
    "Return ONLY valid JSON.",
    "Schema:",
    "{",
    '  "thought": string,',
    '  "tool": "tap_element"|"mobile_type_keys"|"mobile_launch_app"|"mobile_press_button"|null,',
    '  "args": object,',
    '  "done": boolean',
    "}",
    "",
    "Tool args requirements:",
    '- tap_element: { "index": number }',
    '- mobile_type_keys: { "text": string }',
    '- mobile_launch_app: { "app_id": string } // Android package name',
    '- mobile_press_button: { "button": "HOME"|"BACK"|"ENTER" }',
    "",
    "Generic UI rules (NOT app-specific):",
    "- Distinguish intents:",
    "  * SEARCH intent: use a search UI, type query into a search input, then ENTER.",
    "  * CREATE intent: first enter creation mode via Create/New/Add/Compose button, then type into editor input(s).",
    "- Not every visible input is an editor; many apps show a search bar on the home screen.",
    "- Only use mobile_type_keys when an input is visible AND it matches the current intent (search vs editor).",
    "- Avoid tapping the same element index repeatedly if it didn't change the screen.",
    "",
    "If required args are missing, set tool to null and explain in thought.",
    "If you provide a tool, set done=false. Only set done=true when no further actions are needed.",
    "",
    `Goal: ${goal}`,
    "",
    `Hints: ${hints || "none"}`,
    "",
    "Screen elements (actionable list with indices):",
    screenContext || "(none)",
    "",
    "History (most recent last):",
    JSON.stringify(history || []),
  ].join("\n");

  const raw = await llm.chat(prompt);
  try {
    return JSON.parse(raw);
  } catch (e) {
    print("Failed to parse LLM response as JSON.");
    print(raw);
    return null;
  }
}

/**
 * 1. Robust Device Detection
 */
async function initializeDevice() {
  try {
    const output = await run_command("adb devices");
    const lines = output.split("\n");
    for (let line of lines) {
      const match = line.match(/^(\S+)\s+device/);
      if (match) {
        DEVICE_ID = match[1];
        print(`Connected to: ${DEVICE_ID}`);
        return true;
      }
    }
    print("No active device found in 'adb devices' output.");
    print("Raw adb devices output:");
    print(output);
    return false;
  } catch (e) {
    print(`Failed to run adb devices: ${e}`);
    return false;
  }
}

async function adb(cmd) {
  if (!DEVICE_ID) throw new Error("Device not initialized");
  const fullCmd = `adb -s ${DEVICE_ID} ${cmd}`;
  print(`ADB: ${fullCmd}`);
  return await run_command(fullCmd);
}

/**
 * Parses ADB XML dump and returns actionable elements.
 * Populates UI_CACHE with richer metadata (resId/isEdit).
 */
function parseAdbXml(xmlString) {
  UI_CACHE = [];
  const elements = [];
  const nodeRegex = /<node\b[^>]*>/g;

  let match;
  let index = 0;

  while ((match = nodeRegex.exec(xmlString)) !== null) {
    const node = match[0];
    const attrs = {};
    const attrRegex = /([\w-]+)="([^"]*)"/g;

    let attrMatch;
    while ((attrMatch = attrRegex.exec(node)) !== null) {
      attrs[attrMatch[1]] = attrMatch[2];
    }

    const bounds = attrs["bounds"] || "";
    const b = bounds.match(/\[(\d+),(\d+)\]\[(\d+),(\d+)\]/);
    if (!b) continue;

    const x1 = parseInt(b[1], 10);
    const y1 = parseInt(b[2], 10);
    const x2 = parseInt(b[3], 10);
    const y2 = parseInt(b[4], 10);
    const cx = Math.floor((x1 + x2) / 2);
    const cy = Math.floor((y1 + y2) / 2);

    const text = attrs["text"] || "";
    const resId = attrs["resource-id"] || "";
    const contentDesc = attrs["content-desc"] || "";
    const klass = attrs["class"] || "";

    const clickable = attrs["clickable"] === "true";
    const focusable = attrs["focusable"] === "true";
    const longClickable = attrs["long-clickable"] === "true";
    const enabled = attrs["enabled"] !== "false";
    const isEdit = /EditText/i.test(klass);

    const label =
      text || contentDesc || (resId ? resId.split("/").pop() : "") || "Unknown";

    const actionable = enabled && (clickable || focusable || longClickable || isEdit);
    if (!actionable) continue;

    UI_CACHE[index] = {
      x: cx,
      y: cy,
      desc: label,
      text: text,
      contentDesc: contentDesc,
      resId: resId,
      klass: klass,
      isEdit: isEdit,
      clickable: clickable,
      focusable: focusable,
      longClickable: longClickable,
      bounds: bounds,
    };

    const flags = [
      clickable ? "clickable" : null,
      focusable ? "focusable" : null,
      longClickable ? "long-clickable" : null,
      isEdit ? "input" : null,
    ]
      .filter(Boolean)
      .join(", ");

    elements.push(
      `[${index}] "${label}" (${flags || "actionable"}) ` +
        `class="${klass || "?"}" id="${resId || "?"}" desc="${contentDesc || "?"}"`
    );

    index++;
  }

  return elements.join("\n") || "No actionable elements found (screen locked or empty).";
}

function findElementIndexByLabel(substring) {
  if (!substring) return -1;
  const needle = normalize(substring);
  for (let i = 0; i < UI_CACHE.length; i++) {
    const d = normalize(UI_CACHE[i] && UI_CACHE[i].desc);
    const t = normalize(UI_CACHE[i] && UI_CACHE[i].text);
    const c = normalize(UI_CACHE[i] && UI_CACHE[i].contentDesc);
    if (d.includes(needle) || t.includes(needle) || c.includes(needle)) return i;
  }
  return -1;
}

function findElementIndexById(substring) {
  // FIXED: search resource-id, not desc
  if (!substring) return -1;
  const needle = normalize(substring);
  for (let i = 0; i < UI_CACHE.length; i++) {
    const rid = normalize(UI_CACHE[i] && UI_CACHE[i].resId);
    if (rid.includes(needle)) return i;
  }
  return -1;
}

async function getPackageCandidates(goal) {
  const keywords = extractGoalKeywords(goal);
  if (keywords.length === 0) return [];
  const raw = await adb("shell pm list packages -3");
  const packages = raw
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.startsWith("package:"))
    .map((l) => l.slice("package:".length));
  const matches = packages.filter((p) => keywords.some((k) => p.includes(k)));
  return matches.slice(0, 20);
}

// --- CORE LOGIC ---
async function runStep(goal, history) {
  // 1) Capture screen
  await adb("shell uiautomator dump /sdcard/view.xml");
  const xmlData = await adb("shell cat /sdcard/view.xml");
  const screenContext = parseAdbXml(xmlData);

  print("Screen elements:");
  print(screenContext);

  // Intent-aware guardrails (app-agnostic)
  const searchQuery = extractSearchQuery(goal);
  const createText = extractCreateText(goal);

  const inputIdx = firstInputIndex();
  const inputCount = countInputs();
  const searchy = screenLooksSearchy();

  // CREATE intent: prefer tapping a create/new/add affordance before typing,
  // so we don't type into a home-screen search bar.
  if (createText && !historyHasTyped(history, createText)) {
    const createIdx = findCreateAffordanceIndex();
    // If we are not already in a likely editor screen, try entering create mode.
    // Heuristic: if the screen looks searchy and has an input, that input is likely search, not editor.
    const likelySearchBar = (inputIdx !== -1 && searchy);

    if (createIdx !== -1 && !historyJustTapped(history, createIdx) && likelySearchBar) {
      const target = UI_CACHE[createIdx];
      print(`Guardrail(CREATE): tapping create affordance index=${createIdx} desc="${target.desc}"`);
      await adb(`shell input tap ${target.x} ${target.y}`);
      return { tool: "tap_element", args: { index: createIdx }, thought: "Enter create mode", done: false };
    }

    // If we see input(s) and we are NOT in a searchy screen, treat it as editor and type.
    if (inputIdx !== -1 && !searchy) {
      const t = UI_CACHE[inputIdx];
      print(`Guardrail(CREATE): editor-like input visible; typing "${createText}"`);
      await adb(`shell input tap ${t.x} ${t.y}`);
      await adb(`shell input text ${escapeAdbText(createText)}`);
      // Do NOT press ENTER universally for editor text; many apps use ENTER/newline.
      return { tool: "mobile_type_keys", args: { text: createText }, thought: "Type content into editor", done: true };
    }
  }

  // SEARCH intent: only auto-type when the screen looks like a search UI.
  if (searchQuery && inputIdx !== -1 && searchy && !historyHasTyped(history, searchQuery)) {
    const t = UI_CACHE[inputIdx];
    print(`Guardrail(SEARCH): search input visible; typing "${searchQuery}" then ENTER.`);
    await adb(`shell input tap ${t.x} ${t.y}`);
    await adb(`shell input text ${escapeAdbText(searchQuery)}`);
    await adb("shell input keyevent 66"); // ENTER
    return { tool: "mobile_type_keys", args: { text: searchQuery }, thought: "Type search query and submit", done: true };
  }

  // 2) Plan with LLM
  let hints = "";
  const candidates = await getPackageCandidates(goal);
  if (candidates.length > 0) {
    hints += "Possible app packages from device (filtered by goal keywords):\n- " + candidates.join("\n- ");
  }

  if (searchQuery) hints += (hints ? "\n" : "") + `Intent: SEARCH, query="${searchQuery}"`;
  if (createText) hints += (hints ? "\n" : "") + `Intent: CREATE, text="${createText}"`;

  hints += (hints ? "\n" : "") + `UI: inputs=${inputCount}, first_input_index=${inputIdx}, looks_searchy=${searchy}`;

  const plan = await ask_llm(goal, history, screenContext, hints || "none");
  if (!plan) return { error: "LLM Failed" };

  print(`Action: ${plan.thought}`);

  // 3) Execute
  if (plan.done && !plan.tool) return plan;

  if (plan.tool === "tap_element") {
    const idx = plan.args && typeof plan.args.index === "number" ? plan.args.index : null;
    if (idx === null) {
      print("Error: tap_element missing index.");
      return { error: "missing_index" };
    }

    // Minimal anti-loop recovery: if it's repeating the same tap, try BACK once.
    if (wouldRepeatSameTap(history, idx)) {
      print(`Guard: repeated tap on index ${idx}; pressing BACK to recover.`);
      await adb("shell input keyevent 4");
      return {
        tool: "mobile_press_button",
        args: { button: "BACK" },
        thought: "Avoid repeated tap loop; recover with BACK",
        done: false,
      };
    }

    const target = UI_CACHE[idx];
    if (target) await adb(`shell input tap ${target.x} ${target.y}`);
  } else if (plan.tool === "mobile_type_keys") {
    const text = plan.args && plan.args.text ? String(plan.args.text) : "";
    if (!text) {
      print("Error: mobile_type_keys missing text.");
      return { error: "missing_text" };
    }

    // Safety: only type if an input is visible
    const idx = firstInputIndex();
    if (idx === -1) {
      print("Guard: no input visible; refusing to type.");
      return {
        tool: null,
        args: {},
        thought: "No input visible; must reveal/focus an input field before typing.",
        done: false,
      };
    }

    const t = UI_CACHE[idx];
    await adb(`shell input tap ${t.x} ${t.y}`);
    await adb(`shell input text ${escapeAdbText(text)}`);
  } else if (plan.tool === "mobile_launch_app") {
    const appId = plan.args && plan.args.app_id;
    if (!appId) {
      print("Error: mobile_launch_app missing app_id. Provide Android package name.");
      return { error: "missing_app_id" };
    }
    await adb(`shell monkey -p ${appId} -c android.intent.category.LAUNCHER 1`);
  } else if (plan.tool === "mobile_press_button") {
    const keys = { HOME: 3, BACK: 4, ENTER: 66 };
    const b = plan.args && plan.args.button ? plan.args.button : "HOME";
    await adb(`shell input keyevent ${keys[b] || 3}`);
  }

  return plan;
}

export default async function main(input) {
  const ready = await initializeDevice();
  if (!ready) return;

  let goal = "";
  if (input && input.args) {
    const idx = input.args.indexOf("--goal");
    if (idx !== -1) goal = input.args[idx + 1];
  }

  if (!goal) {
    print("Error: No --goal provided.");
    return;
  }

  print(`Starting mission: ${goal}`);

  const history = [];
  for (let i = 0; i < 15; i++) {
    const result = await runStep(goal, history);
    if (result && result.done) {
      print("Success: Goal achieved.");
      break;
    }
    history.push(result);

    // Give UI time to settle
    sleep(1200);
  }
}