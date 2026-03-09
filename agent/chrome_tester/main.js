import { createConfig } from "./config.js";
import { createActions } from "./actions.js";
import { SYSTEM_PROMPT, getDecision, trimMessages } from "./llm-providers.js";
import { SessionLogger } from "./logger.js";
import { runWorkflow } from "./workflow.js";
import { nowMs, sleep } from "./runtime.js";

function parseCliArgs(args) {
  const out = {
    goal: null,
    startUrl: null,
    workflow: null,
    maxSteps: null,
    config: {},
  };

  for (let i = 0; i < args.length; i++) {
    const a = String(args[i]);
    if (a === "--goal" && args[i + 1]) out.goal = String(args[++i]);
    else if (a.startsWith("--goal=")) out.goal = a.slice("--goal=".length);
    else if (a === "--start-url" && args[i + 1]) out.startUrl = String(args[++i]);
    else if (a.startsWith("--start-url=")) out.startUrl = a.slice("--start-url=".length);
    else if (a === "--workflow" && args[i + 1]) out.workflow = String(args[++i]);
    else if (a.startsWith("--workflow=")) out.workflow = a.slice("--workflow=".length);
    else if (a === "--max-steps" && args[i + 1]) out.maxSteps = Number(args[++i]);
    else if (a.startsWith("--max-steps=")) out.maxSteps = Number(a.slice("--max-steps=".length));
    else if (a === "--step-delay" && args[i + 1]) out.config.STEP_DELAY = Number(args[++i]);
    else if (a.startsWith("--step-delay=")) out.config.STEP_DELAY = Number(a.slice("--step-delay=".length));
  }

  return out;
}

function buildUserPrompt(goal, startUrl, lastActionResult, stateSummary, step, stuckCount, capSummary) {
  const lines = [];
  lines.push(`GOAL: ${goal}`);
  if (startUrl) lines.push(`START_URL: ${startUrl}`);
  lines.push(`STEP: ${step}`);
  if (lastActionResult) lines.push(`LAST_ACTION_RESULT: ${lastActionResult}`);
  if (stuckCount > 0) lines.push(`STUCK_COUNT: ${stuckCount}`);
  if (stuckCount >= 2) lines.push("WARNING: state has not changed for multiple steps; switch strategy.");
  lines.push("\nAVAILABLE_CAPABILITIES:");
  lines.push(capSummary);
  lines.push("\nBROWSER_STATE:");
  lines.push(stateSummary);
  return lines.join("\n");
}

function extractPhonesFromResult(result) {
  const out = [];
  const seen = {};
  const phoneRe = /(?:\+?\d[\d\s().-]{6,}\d)/g;

  function push(raw) {
    const s = String(raw || "").trim();
    if (!s) return;
    const digits = s.replace(/[^\d]/g, "");
    if (digits.length < 7) return;
    const key = s.replace(/[\s().-]/g, "");
    if (seen[key]) return;
    seen[key] = true;
    out.push(s);
  }

  function walk(v) {
    if (v == null) return;
    if (typeof v === "string") {
      const m = v.match(phoneRe) || [];
      for (const x of m) push(x);
      return;
    }
    if (Array.isArray(v)) {
      for (const x of v) walk(x);
      return;
    }
    if (typeof v === "object") {
      for (const val of Object.values(v)) walk(val);
    }
  }

  walk(result);
  return out;
}

async function executeDecision(actions, decision) {
  switch (decision.action) {
    case "navigate": return actions.navigate(decision.url);
    case "snapshot": return actions.snapshot();
    case "list_pages": return actions.listPages();
    case "select_page": return actions.selectPage(Number(decision.index));
    case "click_uid": return actions.click(decision.uid);
    case "fill_uid": return actions.fill(decision.uid, decision.text);
    case "type_text": return actions.typeText(decision.text);
    case "press_key": return actions.pressKey(decision.key);
    case "wait_for_text": return actions.waitForText(decision.text, decision.timeoutMs);
    case "evaluate": return actions.evaluate(decision.script);
    case "screenshot": return actions.screenshot(decision.path || "");
    case "detect_blocking_overlay": return actions.detectBlockingOverlay();
    case "dismiss_blocking_overlay": return actions.dismissBlockingOverlay();
    case "extract_phone_numbers": return actions.extractPhoneNumbers();
    case "wait":
      await sleep(1500);
      return { success: true, message: "Waited 1.5s" };
    case "done":
      return { success: true, message: "done" };
    default:
      return { success: false, message: `Unsupported action: ${decision.action}` };
  }
}

async function runAgent(goal, startUrl, maxSteps, config, actions) {
  const logger = new SessionLogger(config.LOG_DIR, goal);
  const messages = [{ role: "system", content: SYSTEM_PROMPT }];
  let lastActionResult = "";
  let prevHash = "";
  let stuckCount = 0;
  let lastPhone = "";

  if (startUrl) {
    const initialNav = await actions.navigate(startUrl);
    lastActionResult = `navigate(${startUrl}) -> ${initialNav.success ? "OK" : "FAILED"}: ${initialNav.message}`;
    print(lastActionResult);
  }

  for (let step = 1; step <= maxSteps; step++) {
    print(`\n--- Step ${step}/${maxSteps} ---`);

    const state = await actions.captureState();
    const stateHash = state.hash || "";
    if (prevHash && stateHash && prevHash === stateHash) stuckCount++;
    else stuckCount = 0;
    prevHash = stateHash;

    const userPrompt = buildUserPrompt(
      goal,
      startUrl,
      lastActionResult,
      state.summary,
      step,
      stuckCount,
      actions.capabilitySummary(),
    );

    messages.push({ role: "user", content: userPrompt });
    const trimmed = trimMessages(messages, config.MAX_HISTORY_STEPS);

    const llmStart = nowMs();
    let decision;
    try {
      decision = await getDecision(trimmed);
    } catch (err) {
      decision = { action: "wait", reason: `LLM call failed: ${String(err && err.message ? err.message : err)}` };
    }
    const llmLatency = nowMs() - llmStart;
    messages.push({ role: "assistant", content: JSON.stringify(decision) });

    print(`Decision: ${decision.action} - ${decision.reason || "no reason"} (${Math.round(llmLatency)}ms)`);

    const actionStart = nowMs();
    let result;
    try {
      result = await executeDecision(actions, decision);
    } catch (err2) {
      result = { success: false, message: String(err2 && err2.message ? err2.message : err2) };
    }
    const actionLatency = nowMs() - actionStart;

    logger.logStep(
      step,
      stateHash,
      state.summary,
      decision,
      result,
      Math.round(llmLatency),
      Math.round(actionLatency),
    );

    lastActionResult = `${decision.action} -> ${result.success ? "OK" : "FAILED"}: ${result.message}`;
    print(lastActionResult);

    const phones = extractPhonesFromResult(result);
    if (phones.length) {
      lastPhone = phones[0];
      print(`PHONE_CANDIDATES: ${phones.join(" | ")}`);
    }

    if (decision.action === "done") {
      if (lastPhone) {
        print(`PHONE_NUMBER: ${lastPhone}`);
      }
      logger.finalize(true, { stepsUsed: step });
      return { success: true, stepsUsed: step, phoneNumber: lastPhone || null };
    }

    const stepDelayMs = Math.max(0, Number(config.STEP_DELAY || 0) * 1000);
    const remainingDelayMs = stepDelayMs - actionLatency;
    if (remainingDelayMs > 0) await sleep(remainingDelayMs);
  }

  logger.finalize(false, { stepsUsed: maxSteps });
  return { success: false, stepsUsed: maxSteps, phoneNumber: lastPhone || null };
}

export default async function main(runInput) {
  const args = (runInput && Array.isArray(runInput.args)) ? runInput.args : [];
  const parsed = parseCliArgs(args);
  const config = createConfig(parsed.config);
  const actions = createActions(config);

  const caps = await actions.discoverCapabilities();
  if (!caps.success) {
    const out = { success: false, error: caps.message };
    print(JSON.stringify(out, null, 2));
    return out;
  }

  print(caps.message);
  print("Capabilities:");
  print(actions.capabilitySummary());

  if (parsed.workflow) {
    if (!fs.exists(parsed.workflow)) {
      const out = { success: false, error: `Workflow file not found: ${parsed.workflow}` };
      print(JSON.stringify(out, null, 2));
      return out;
    }

    let workflow;
    try {
      workflow = JSON.parse(fs.read_text(parsed.workflow));
    } catch (err) {
      const out = {
        success: false,
        error: `Failed to parse workflow JSON: ${String(err && err.message ? err.message : err)}`,
      };
      print(JSON.stringify(out, null, 2));
      return out;
    }

    const result = await runWorkflow(
      workflow,
      async (goal, startUrl, maxSteps) => runAgent(goal, startUrl, maxSteps, config, actions),
    );
    print(`Workflow result: ${result.success ? "OK" : "FAILED"} (${result.steps.filter((x) => x.success).length}/${result.steps.length} steps passed)`);
    return result;
  }

  let goal = parsed.goal;
  if (!goal && runInput && typeof runInput.goal === "string") goal = runInput.goal;
  if (!goal) goal = (await input("Enter testing goal: ")).trim();
  if (!goal) return { success: false, error: "No goal provided" };

  const steps = Number.isFinite(parsed.maxSteps) && parsed.maxSteps > 0
    ? parsed.maxSteps
    : config.MAX_STEPS;

  return runAgent(goal, parsed.startUrl || "", steps, config, actions);
}
