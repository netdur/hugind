import { sleep } from "./runtime.js";
import { getInteractiveElements } from "./sanitizer.js";

function parseScalar(v) {
  const s = v.trim();
  if ((s.startsWith('"') && s.endsWith('"')) || (s.startsWith("'") && s.endsWith("'"))) return s.slice(1, -1);
  if (/^-?\d+(\.\d+)?$/.test(s)) return Number(s);
  return s;
}

function parseInlinePair(s) {
  const i = s.indexOf(":");
  if (i === -1) return s.trim();
  const k = s.slice(0, i).trim();
  const rawV = s.slice(i + 1).trim();
  if (rawV.startsWith("[") && rawV.endsWith("]")) {
    return { [k]: rawV.slice(1, -1).split(",").map((x) => Number(x.trim())) };
  }
  return { [k]: parseScalar(rawV) };
}

function parseFlowFile(path) {
  if (!fs.exists(path)) throw new Error(`Flow file not found: ${path}`);
  const raw = fs.read_text(path);
  const lines = raw.split(/\r?\n/);
  let inSteps = false;
  const frontmatter = {};
  const steps = [];

  for (let line of lines) {
    const t = line.trim();
    if (!t || t.startsWith("#")) continue;
    if (t === "---") {
      inSteps = true;
      continue;
    }
    if (!inSteps && t.includes(":")) {
      const idx = t.indexOf(":");
      frontmatter[t.slice(0, idx).trim()] = parseScalar(t.slice(idx + 1));
      continue;
    }
    if (t.startsWith("- ")) {
      const body = t.slice(2).trim();
      steps.push(parseInlinePair(body));
    }
  }

  if (!steps.length) throw new Error("Flow file contains no steps");
  return { frontmatter, steps };
}

async function scanScreen(config, actions) {
  await actions.runAdbCommand(["shell", "uiautomator", "dump", config.SCREEN_DUMP_PATH]);
  await actions.runAdbCommand(["pull", config.SCREEN_DUMP_PATH, config.LOCAL_DUMP_PATH]);
  if (!fs.exists(config.LOCAL_DUMP_PATH)) return [];
  return getInteractiveElements(fs.read_text(config.LOCAL_DUMP_PATH));
}

function findElementByText(elements, query) {
  const q = String(query).toLowerCase();
  let hit = elements.find((el) => el.text && el.text.toLowerCase() === q);
  if (hit) return hit;
  const matches = elements.filter((el) => el.text && el.text.toLowerCase().indexOf(q) !== -1).sort((a, b) => a.text.length - b.text.length);
  if (matches.length) return matches[0];
  hit = elements.find((el) => el.hint && el.hint.toLowerCase().indexOf(q) !== -1);
  if (hit) return hit;
  hit = elements.find((el) => el.id && el.id.toLowerCase().indexOf(q) !== -1);
  return hit || null;
}

async function executeFlowStep(step, frontmatter, config, actions) {
  if (typeof step === "string") {
    if (step === "launchApp") {
      if (!frontmatter.appId) return { success: false, message: "launchApp requires appId in frontmatter" };
      return actions.executeAction({ action: "launch", package: frontmatter.appId });
    }
    if (["back", "home", "enter", "clear", "done"].includes(step)) {
      return actions.executeAction({ action: step === "done" ? "done" : step, reason: "Flow complete" });
    }
    return { success: false, message: `Unknown step: ${step}` };
  }

  const cmd = Object.keys(step)[0];
  const value = step[cmd];

  if (cmd === "tap") {
    if (Array.isArray(value)) return actions.executeAction({ action: "tap", coordinates: value });
    const el = findElementByText(await scanScreen(config, actions), value);
    if (!el) return { success: false, message: `Element "${value}" not found` };
    return actions.executeAction({ action: "tap", coordinates: el.center });
  }
  if (cmd === "longpress") {
    if (Array.isArray(value)) return actions.executeAction({ action: "longpress", coordinates: value });
    const el = findElementByText(await scanScreen(config, actions), value);
    if (!el) return { success: false, message: `Element "${value}" not found` };
    return actions.executeAction({ action: "longpress", coordinates: el.center });
  }
  if (cmd === "type") return actions.executeAction({ action: "type", text: String(value) });
  if (cmd === "swipe") return actions.executeAction({ action: "swipe", direction: String(value) });
  if (cmd === "scroll") return actions.executeAction({ action: "scroll", direction: String(value) });
  if (cmd === "wait") {
    const seconds = Number(value) || 2;
    await sleep(seconds * 1000);
    return { success: true, message: `Waited ${seconds}s` };
  }
  if (cmd === "launch") return actions.executeAction({ action: "launch", package: String(value) });
  if (cmd === "openUrl") return actions.executeAction({ action: "open_url", url: String(value) });
  if (cmd === "clipboard") return actions.executeAction({ action: "clipboard_set", text: String(value) });
  if (cmd === "paste") return actions.executeAction({ action: "paste", coordinates: Array.isArray(value) ? value : undefined });
  if (cmd === "shell") return actions.executeAction({ action: "shell", command: String(value) });
  if (cmd === "keyevent") return actions.executeAction({ action: "keyevent", code: Number(value) });
  if (cmd === "settings") return actions.executeAction({ action: "open_settings", setting: String(value) });
  if (cmd === "done") return actions.executeAction({ action: "done", reason: String(value) });

  return { success: false, message: `Unknown command: ${cmd}` };
}

export async function runFlow(path, config, actions) {
  const parsed = parseFlowFile(path);
  const name = parsed.frontmatter.name || path.split("/").pop() || "flow";

  for (let i = 0; i < parsed.steps.length; i++) {
    const r = await executeFlowStep(parsed.steps[i], parsed.frontmatter, config, actions);
    if (!r.success) {
      return { name, success: false, stepsCompleted: i, totalSteps: parsed.steps.length, error: r.message };
    }
    if (i < parsed.steps.length - 1) await sleep(config.STEP_DELAY * 1000);
  }

  return { name, success: true, stepsCompleted: parsed.steps.length, totalSteps: parsed.steps.length };
}
