import { createConfig } from "./config.js";
import { createActions, initDeviceContext } from "./actions.js";
import { createSkills } from "./skills.js";
import { getInteractiveElements, computeScreenHash, filterElements } from "./sanitizer.js";
import { getDecision, trimMessages, SYSTEM_PROMPT } from "./llm-providers.js";
import { SessionLogger } from "./logger.js";
import { runWorkflow } from "./workflow.js";
import { runFlow } from "./flow.js";
import { DEVICE_SCREENSHOT_PATH, LOCAL_SCREENSHOT_PATH } from "./constants.js";
import { sleep, bytesToBase64, nowMs } from "./runtime.js";

function parseCliArgs(args) {
  const out = { goal: null, flow: null, workflow: null, maxSteps: null, config: {} };
  for (let i = 0; i < args.length; i++) {
    const a = String(args[i]);
    if (a === "--goal" && args[i + 1]) out.goal = String(args[++i]);
    else if (a.startsWith("--goal=")) out.goal = a.slice("--goal=".length);
    else if (a === "--flow" && args[i + 1]) out.flow = String(args[++i]);
    else if (a.startsWith("--flow=")) out.flow = a.slice("--flow=".length);
    else if (a === "--workflow" && args[i + 1]) out.workflow = String(args[++i]);
    else if (a.startsWith("--workflow=")) out.workflow = a.slice("--workflow=".length);
    else if (a === "--max-steps" && args[i + 1]) out.maxSteps = Number(args[++i]);
    else if (a.startsWith("--max-steps=")) out.maxSteps = Number(a.slice("--max-steps=".length));
    else if (a === "--step-delay" && args[i + 1]) out.config.STEP_DELAY = Number(args[++i]);
    else if (a.startsWith("--step-delay=")) out.config.STEP_DELAY = Number(a.slice("--step-delay=".length));
    else if (a === "--vision" && args[i + 1]) out.config.VISION_MODE = String(args[++i]);
    else if (a.startsWith("--vision=")) out.config.VISION_MODE = a.slice("--vision=".length);
    else if (a === "--streaming" && args[i + 1]) out.config.STREAMING_ENABLED = String(args[++i]) === "true";
    else if (a.startsWith("--streaming=")) out.config.STREAMING_ENABLED = a.slice("--streaming=".length) === "true";
  }
  return out;
}

function diffScreenState(prevElements, currElements) {
  const prevTexts = {};
  const currTexts = {};
  for (const e of prevElements) if (e.text) prevTexts[e.text] = true;
  for (const e of currElements) if (e.text) currTexts[e.text] = true;

  const added = [];
  const removed = [];
  for (const t of Object.keys(currTexts)) if (!prevTexts[t]) added.push(t);
  for (const t of Object.keys(prevTexts)) if (!currTexts[t]) removed.push(t);

  const changed = computeScreenHash(prevElements) !== computeScreenHash(currElements);
  let summary = "";
  if (!changed) summary = "Screen has NOT changed since last action.";
  else {
    const parts = [];
    if (added.length) parts.push(`New on screen: ${added.slice(0, 5).join(', ')}`);
    if (removed.length) parts.push(`Gone from screen: ${removed.slice(0, 5).join(', ')}`);
    summary = parts.join(". ") || "Screen layout changed.";
  }
  return { changed, summary };
}

function buildUserContent(goal, foregroundApp, lastActionFeedback, screenContext, diffContext, visionContext, screenshotBase64) {
  const foregroundLine = foregroundApp ? `FOREGROUND_APP: ${foregroundApp}\n\n` : "";
  const actionFeedbackLine = lastActionFeedback ? `LAST_ACTION_RESULT: ${lastActionFeedback}\n\n` : "";
  const text = `GOAL: ${goal}\n\n${foregroundLine}${actionFeedbackLine}SCREEN_CONTEXT:\n${screenContext}${diffContext}${visionContext}`;
  const parts = [{ type: "text", text }];
  if (screenshotBase64) {
    parts.push({ type: "image", base64: screenshotBase64, mimeType: "image/png" });
  }
  return parts;
}

export default async function main(runInput) {
  const args = (runInput && Array.isArray(runInput.args)) ? runInput.args : [];
  const parsed = parseCliArgs(args);
  const config = createConfig(parsed.config);
  const actions = createActions(config);
  const skills = createSkills(config, actions);

  async function getScreenState() {
    try {
      await actions.runAdbCommand(["shell", "uiautomator", "dump", config.SCREEN_DUMP_PATH]);
      await actions.runAdbCommand(["pull", config.SCREEN_DUMP_PATH, config.LOCAL_DUMP_PATH]);
    } catch (_err) {
      return { elements: [], compactJson: "Error: Could not capture screen." };
    }

    if (!fs.exists(config.LOCAL_DUMP_PATH)) {
      return { elements: [], compactJson: "Error: Could not capture screen." };
    }

    const xml = fs.read_text(config.LOCAL_DUMP_PATH);
    const elements = getInteractiveElements(xml);
    const compact = filterElements(elements, config.MAX_ELEMENTS);
    return { elements, compactJson: JSON.stringify(compact) };
  }

  async function captureScreenshotBase64() {
    try {
      await actions.runAdbCommand(["shell", "screencap", "-p", DEVICE_SCREENSHOT_PATH]);
      await actions.runAdbCommand(["pull", DEVICE_SCREENSHOT_PATH, LOCAL_SCREENSHOT_PATH]);
      if (fs.exists(LOCAL_SCREENSHOT_PATH)) {
        return bytesToBase64(fs.read_bytes(LOCAL_SCREENSHOT_PATH));
      }
    } catch (_err) {}
    return null;
  }

  async function runAgent(goal, maxSteps) {
    const steps = maxSteps || config.MAX_STEPS;
    const resolution = await actions.getScreenResolution();
    if (resolution) {
      initDeviceContext(resolution);
      print(`Screen resolution: ${resolution[0]}x${resolution[1]}`);
    }

    const logger = new SessionLogger(config.LOG_DIR, goal);
    const messages = [{ role: "system", content: SYSTEM_PROMPT }];
    const multiStepActions = { read_screen: true, submit_message: true, copy_visible_text: true, wait_for_content: true, find_and_tap: true, compose_email: true };

    let prevElements = [];
    let stuckCount = 0;
    let recentActions = [];
    let lastActionFeedback = "";

    for (let step = 0; step < steps; step++) {
      print(`\n--- Step ${step + 1}/${steps} ---`);
      const state = await getScreenState();
      const foregroundApp = await actions.getForegroundApp();

      let diffContext = "";
      let screenChanged = true;
      if (step > 0) {
        const diff = diffScreenState(prevElements, state.elements);
        screenChanged = diff.changed;
        diffContext = `\n\nSCREEN_CHANGE: ${diff.summary}`;
        if (!diff.changed) {
          stuckCount++;
          if (stuckCount >= config.STUCK_THRESHOLD) {
            diffContext += `\nWARNING: You have been stuck for ${stuckCount} steps. The screen is NOT changing.`;
          }
        } else {
          stuckCount = 0;
        }
      }
      prevElements = state.elements;

      let screenshotBase64 = null;
      let visionContext = "";
      const shouldCaptureVision = config.VISION_MODE === "always" || (config.VISION_MODE === "fallback" && state.elements.length === 0) || stuckCount >= 2;
      if (shouldCaptureVision) {
        screenshotBase64 = await captureScreenshotBase64();
        if (state.elements.length === 0) {
          visionContext = "\n\nVISION_FALLBACK: The accessibility tree returned NO elements.";
        } else if (stuckCount >= 2) {
          visionContext = "\n\nVISION_ASSIST: You have been stuck — a screenshot is attached.";
        }
      }

      const userContent = buildUserContent(goal, foregroundApp, lastActionFeedback, state.compactJson, diffContext, visionContext, screenshotBase64);
      messages.push({ role: "user", content: userContent });
      const trimmed = trimMessages(messages, config.MAX_HISTORY_STEPS);

      const llmStart = nowMs();
      let decision;
      try {
        decision = await getDecision(trimmed, config.STREAMING_ENABLED);
      } catch (err) {
        decision = { action: "wait", reason: `LLM request failed: ${String(err && err.message ? err.message : err)}` };
      }
      const llmLatency = nowMs() - llmStart;

      if (decision.think) print(`Think: ${decision.think}`);
      if (decision.plan) print(`Plan: ${decision.plan.join(" -> ")}`);
      if (decision.planProgress) print(`Progress: ${decision.planProgress}`);
      print(`Decision: ${decision.action} - ${decision.reason || "no reason"} (${Math.round(llmLatency)}ms)`);

      messages.push({ role: "assistant", content: JSON.stringify(decision) });

      const actionStart = nowMs();
      let result;
      try {
        if (multiStepActions[decision.action]) result = await skills.executeSkill(decision, state.elements);
        else result = await actions.executeAction(decision);
      } catch (err) {
        result = { success: false, message: String(err && err.message ? err.message : err) };
      }
      const actionLatency = nowMs() - actionStart;

      logger.logStep(step + 1, foregroundApp, state.elements.length, screenChanged, decision, result, Math.round(llmLatency), Math.round(actionLatency));

      const actionSig = decision.coordinates ? `${decision.action}(${decision.coordinates.join(",")})` : decision.action;
      recentActions.push(actionSig);
      if (recentActions.length > 8) recentActions.shift();
      lastActionFeedback = `${actionSig} -> ${result.success ? "OK" : "FAILED"}: ${result.message}`;

      if (decision.action === "done") {
        logger.finalize(true);
        return { success: true, stepsUsed: step + 1 };
      }

      await sleep(config.STEP_DELAY * 1000);
    }

    logger.finalize(false);
    return { success: false, stepsUsed: steps };
  }

  if (parsed.flow) {
    const result = await runFlow(parsed.flow, config, actions);
    print(`Result: ${result.success ? "OK" : "FAILED"} (${result.stepsCompleted}/${result.totalSteps} steps)`);
    return result;
  }

  if (parsed.workflow) {
    const workflow = JSON.parse(fs.read_text(parsed.workflow));
    const result = await runWorkflow(workflow, runAgent, actions.runAdbCommand);
    return result;
  }

  let goal = parsed.goal;
  if (!goal && runInput && typeof runInput.goal === "string") goal = runInput.goal;
  if (!goal) goal = (await input("Enter your goal: ")).trim();
  if (!goal) return { success: false, error: "No goal provided" };

  return runAgent(goal, parsed.maxSteps || undefined);
}
