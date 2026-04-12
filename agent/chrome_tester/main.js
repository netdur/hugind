import { createConfig } from "./config.js";
import { createActions } from "./actions.js";
import { sendSystemPrompt, getDecision } from "./llm-providers.js";
import { SessionLogger } from "./logger.js";
import { runWorkflow } from "./workflow.js";
import { nowMs, sleep } from "./runtime.js";

function parseCliArgs(args) {
  const out = {
    goal: null,
    goalFile: null,
    startUrl: null,
    workflow: null,
    maxSteps: null,
    config: {},
  };

  for (let i = 0; i < args.length; i++) {
    const a = String(args[i]);
    if (a === "--goal" && args[i + 1]) out.goal = String(args[++i]);
    else if (a.startsWith("--goal=")) out.goal = a.slice("--goal=".length);
    else if (a === "--goal-file" && args[i + 1]) out.goalFile = String(args[++i]);
    else if (a.startsWith("--goal-file=")) out.goalFile = a.slice("--goal-file=".length);
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

function buildFirstPrompt(goal, startUrl, stateSummary, capSummary) {
  const lines = [];
  lines.push(`GOAL: ${goal}`);
  if (startUrl) lines.push(`START_URL: ${startUrl}`);
  lines.push("\nAVAILABLE_CAPABILITIES:");
  lines.push(capSummary);
  lines.push("\nBROWSER_STATE:");
  lines.push(stateSummary);
  return lines.join("\n");
}

function buildStepPrompt(lastActionResult, stateSummary, step, stuckCount, goal) {
  const lines = [];
  lines.push(`GOAL: ${goal}`);
  lines.push(`STEP: ${step}`);
  lines.push(`LAST_ACTION_RESULT: ${lastActionResult}`);
  if (stuckCount >= 2) lines.push("WARNING: state has not changed for multiple steps; switch strategy.");
  lines.push("\nBROWSER_STATE:");
  lines.push(stateSummary);
  return lines.join("\n");
}


function countPages(pagesResult) {
  if (!pagesResult || !pagesResult.success) return 0;
  const lines = String(pagesResult.summary || "").split("\n");
  let count = 0;
  for (const line of lines) {
    if (/^\s*\d+:/.test(line)) count++;
  }
  return count;
}

async function executeDecision(actions, decision) {
  // For click/press_key, detect if a new tab opens and auto-switch
  const mayOpenTab = decision.action === "click_uid" || decision.action === "press_key";
  let pagesBefore = 0;
  if (mayOpenTab) {
    const lp = await actions.listPages();
    pagesBefore = countPages(lp);
  }

  let result;
  switch (decision.action) {
    case "navigate": result = await actions.navigate(decision.url); break;
    case "snapshot": result = await actions.snapshot(); break;
    case "list_pages": result = await actions.listPages(); break;
    case "select_page": result = await actions.selectPage(Number(decision.index)); break;
    case "click_uid": result = await actions.click(decision.uid); break;
    case "fill_uid": result = await actions.fill(decision.uid, decision.text); break;
    case "type_text": result = await actions.typeText(decision.text); break;
    case "press_key": result = await actions.pressKey(decision.key); break;
    case "wait_for_text": result = await actions.waitForText(decision.text, decision.timeoutMs); break;
    case "evaluate": result = await actions.evaluate(decision.script); break;
    case "screenshot": result = await actions.screenshot(decision.path || ""); break;
    case "scroll": result = await actions.scroll(decision.direction || "down"); break;
    case "full_snapshot": result = actions.getFullSnapshot(); break;
    case "detect_blocking_overlay": result = await actions.detectBlockingOverlay(); break;
    case "dismiss_blocking_overlay": result = await actions.dismissBlockingOverlay(); break;
    case "extract_phone_numbers": result = { success: false, message: "extract_phone_numbers is no longer supported" }; break;
    case "wait":
      await sleep(1500);
      result = { success: true, message: "Waited 1.5s" };
      break;
    case "done":
      result = { success: true, message: "done" };
      break;
    default:
      result = { success: false, message: `Unsupported action: ${decision.action}` };
  }

  // Auto-detect and switch to new tab if one was opened
  if (mayOpenTab && result.success) {
    // Small delay to let the browser actually open the new tab
    await sleep(600);
    const lpAfter = await actions.listPages();
    const pagesAfter = countPages(lpAfter);
    if (pagesAfter > pagesBefore) {
      // Find the highest pageId (newest tab) and switch to it
      const lines = String(lpAfter.summary || "").split("\n");
      let lastId = null;
      for (const line of lines) {
        const m = line.match(/^\s*(\d+):/);
        if (m) lastId = parseInt(m[1], 10);
      }
      if (lastId != null) {
        print(`  New tab detected, switching to pageId=${lastId}`);
        await actions.selectPage(lastId);
        result.message += ` (auto-switched to new tab ${lastId})`;
      }
    }
  }

  return result;
}

async function runAgent(goal, startUrl, maxSteps, config, actions) {
  const logger = new SessionLogger(config.LOG_DIR, goal);
  let lastActionResult = "";
  let prevHash = "";
  let stuckCount = 0;
  // Send system prompt once — server tracks conversation via X-Session-ID
  await sendSystemPrompt();

  if (startUrl) {
    const initialNav = await actions.navigate(startUrl);
    lastActionResult = `navigate(${startUrl}) -> ${initialNav.success ? "OK" : "FAILED"}: ${initialNav.message}`;
    print(lastActionResult);
  }

  for (let step = 1; step <= maxSteps; step++) {
    print(`\n--- Step ${step}/${maxSteps} ---`);

    const stateStart = nowMs();
    const state = await actions.captureState();
    const stateLatency = nowMs() - stateStart;
    const stateHash = state.hash || "";
    if (prevHash && stateHash && prevHash === stateHash) stuckCount++;
    else stuckCount = 0;
    prevHash = stateHash;

    let prompt;
    if (step === 1) {
      prompt = buildFirstPrompt(goal, startUrl, state.summary, actions.capabilitySummary());
    } else {
      prompt = buildStepPrompt(lastActionResult, state.summary, step, stuckCount, goal);
    }

    const llmStart = nowMs();
    let decision;
    try {
      decision = await getDecision(prompt);
    } catch (err) {
      decision = { action: "wait", reason: `LLM call failed: ${String(err && err.message ? err.message : err)}` };
    }
    const llmLatency = nowMs() - llmStart;

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
    // Include action data in result so LLM sees snapshot content
    if (result.summary && (decision.action === "full_snapshot" || decision.action === "snapshot")) {
      lastActionResult += `\n\n${result.summary}`;
    }

    print("############## REPORT ##############");
    print(`  Chrome MCP : ${Math.round(stateLatency + actionLatency)}ms (state=${Math.round(stateLatency)}ms, action=${Math.round(actionLatency)}ms)`);
    print(`  LLM        : ${Math.round(llmLatency)}ms`);
    print(`  Action     : ${decision.action} ${result.success ? "OK" : "FAILED"}`);
    print(`  Reason     : ${decision.reason || "-"}`);
    print("############ / REPORT ##############");

    if (decision.action === "done") {
      logger.finalize(true, { stepsUsed: step });
      return { success: true, stepsUsed: step, reason: decision.reason || "" };
    }

    const stepDelayMs = Math.max(0, Number(config.STEP_DELAY || 0) * 1000);
    const remainingDelayMs = stepDelayMs - actionLatency;
    if (remainingDelayMs > 0) await sleep(remainingDelayMs);
  }

  logger.finalize(false, { stepsUsed: maxSteps });
  return { success: false, stepsUsed: maxSteps };
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

  // Open a fresh working tab (avoids touching user's existing tabs)
  const tabResult = await actions.openFreshTab();
  print(tabResult.message);

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
  if (!goal && parsed.goalFile) {
    if (!fs.exists(parsed.goalFile)) {
      return { success: false, error: `Goal file not found: ${parsed.goalFile}` };
    }
    goal = fs.read_text(parsed.goalFile).trim();
  }
  if (!goal && runInput && typeof runInput.goal === "string") goal = runInput.goal;
  if (!goal) return { success: false, error: "No goal provided. Use --goal or --goal-file." };

  const steps = Number.isFinite(parsed.maxSteps) && parsed.maxSteps > 0
    ? parsed.maxSteps
    : config.MAX_STEPS;

  return runAgent(goal, parsed.startUrl || "", steps, config, actions);
}
