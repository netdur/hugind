const DEFAULT_STEP_LIMIT = 15;

function buildGoal(step) {
  let goal = String(step.goal || "").trim();
  if (!goal) goal = "Inspect page and complete the requested check.";

  if (Array.isArray(step.checks) && step.checks.length > 0) {
    const lines = step.checks.map((x) => `- ${String(x)}`).join("\n");
    goal += `\n\nREQUIRED CHECKS:\n${lines}`;
  }

  if (typeof step.instructions === "string" && step.instructions.trim()) {
    goal += `\n\nEXTRA INSTRUCTIONS:\n${step.instructions.trim()}`;
  }

  return goal;
}

export async function runWorkflow(workflow, runAgent) {
  if (!workflow || !Array.isArray(workflow.steps) || workflow.steps.length === 0) {
    return {
      name: workflow && workflow.name ? workflow.name : "workflow",
      steps: [],
      success: false,
      error: "Workflow has no steps",
    };
  }

  const continueOnFailure = !!workflow.continueOnFailure;
  const results = [];

  for (let i = 0; i < workflow.steps.length; i++) {
    const step = workflow.steps[i] || {};
    const stepName = step.name ? String(step.name) : `step-${i + 1}`;
    const goal = buildGoal(step);
    const startUrl = step.startUrl ? String(step.startUrl) : "";
    const maxSteps = Number.isFinite(step.maxSteps) && step.maxSteps > 0
      ? step.maxSteps
      : DEFAULT_STEP_LIMIT;

    print(`\n=== Workflow Step ${i + 1}/${workflow.steps.length}: ${stepName} ===`);
    if (startUrl) print(`Start URL: ${startUrl}`);

    try {
      const r = await runAgent(goal, startUrl, maxSteps);
      const row = {
        index: i + 1,
        name: stepName,
        goal: step.goal || goal,
        startUrl,
        success: !!r.success,
        stepsUsed: r.stepsUsed || 0,
      };
      results.push(row);

      if (!row.success && !continueOnFailure) {
        return {
          name: workflow.name || "workflow",
          steps: results,
          success: false,
          stoppedAt: i + 1,
        };
      }
    } catch (err) {
      const row = {
        index: i + 1,
        name: stepName,
        goal: step.goal || goal,
        startUrl,
        success: false,
        stepsUsed: 0,
        error: String(err && err.message ? err.message : err),
      };
      results.push(row);

      if (!continueOnFailure) {
        return {
          name: workflow.name || "workflow",
          steps: results,
          success: false,
          stoppedAt: i + 1,
        };
      }
    }
  }

  return {
    name: workflow.name || "workflow",
    steps: results,
    success: results.every((x) => x.success),
  };
}

