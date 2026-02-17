import { randId, writeJson } from "./runtime.js";

export class SessionLogger {
  constructor(logDir, goal) {
    this.sessionId = `${Date.now()}-${randId()}`;
    this.logDir = logDir;
    this.goal = goal;
    this.startTime = new Date().toISOString();
    this.steps = [];
    if (!fs.exists(logDir)) fs.mkdir(logDir, true);
  }

  buildSummary(completed) {
    return {
      sessionId: this.sessionId,
      goal: this.goal,
      startTime: this.startTime,
      endTime: new Date().toISOString(),
      totalSteps: this.steps.length,
      successCount: this.steps.filter((s) => s.actionResult.success).length,
      failCount: this.steps.filter((s) => !s.actionResult.success).length,
      completed,
      steps: this.steps,
    };
  }

  logStep(step, foregroundApp, elementCount, screenChanged, decision, result, llmLatencyMs, actionLatencyMs) {
    this.steps.push({
      step,
      timestamp: new Date().toISOString(),
      foregroundApp,
      elementCount,
      screenChanged,
      llmDecision: {
        action: decision.action,
        reason: decision.reason,
        coordinates: decision.coordinates,
        text: decision.text,
        think: decision.think,
        plan: decision.plan,
        planProgress: decision.planProgress,
      },
      actionResult: { success: result.success, message: result.message },
      llmLatencyMs,
      actionLatencyMs,
    });

    writeJson(`${this.logDir}/${this.sessionId}.partial.json`, this.buildSummary(false));
  }

  finalize(completed) {
    const out = `${this.logDir}/${this.sessionId}.json`;
    writeJson(out, this.buildSummary(completed));
    print(`Session log saved: ${out}`);
  }
}
