import { sleep } from "./runtime.js";

const DEFAULT_STEP_LIMIT = 15;
const APP_LAUNCH_DELAY_MS = 2000;

function buildGoal(step) {
  let goal = step.goal;
  if (step.formData && Object.keys(step.formData).length > 0) {
    const lines = Object.keys(step.formData).map((k) => `- ${k}: ${step.formData[k]}`).join("\n");
    goal += `\n\nFORM DATA TO FILL:\n${lines}\n\nFind each field on screen and enter the corresponding value.`;
  }
  return goal;
}

export async function runWorkflow(workflow, runAgent, runAdbCommand) {
  const results = [];
  for (let i = 0; i < workflow.steps.length; i++) {
    const step = workflow.steps[i];
    if (step.app) {
      await runAdbCommand(["shell", "monkey", "-p", step.app, "-c", "android.intent.category.LAUNCHER", "1"]);
      await sleep(APP_LAUNCH_DELAY_MS);
    }

    try {
      const r = await runAgent(buildGoal(step), step.maxSteps || DEFAULT_STEP_LIMIT);
      results.push({ goal: step.goal, app: step.app, success: r.success, stepsUsed: r.stepsUsed });
    } catch (err) {
      results.push({ goal: step.goal, app: step.app, success: false, stepsUsed: 0, error: String(err && err.message ? err.message : err) });
    }
  }

  return {
    name: workflow.name,
    steps: results,
    success: results.every((r) => r.success),
  };
}
