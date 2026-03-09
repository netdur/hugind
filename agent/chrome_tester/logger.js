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

  buildSummary(completed, extra) {
    return {
      sessionId: this.sessionId,
      goal: this.goal,
      startTime: this.startTime,
      endTime: new Date().toISOString(),
      totalSteps: this.steps.length,
      completed,
      successCount: this.steps.filter((s) => s.actionResult.success).length,
      failCount: this.steps.filter((s) => !s.actionResult.success).length,
      ...extra,
      steps: this.steps,
    };
  }

  logStep(step, stateHash, stateSummary, decision, result, llmLatencyMs, actionLatencyMs) {
    this.steps.push({
      step,
      timestamp: new Date().toISOString(),
      stateHash,
      stateSummary,
      llmDecision: decision,
      actionResult: {
        success: result.success,
        message: result.message,
      },
      llmLatencyMs,
      actionLatencyMs,
    });

    writeJson(`${this.logDir}/${this.sessionId}.partial.json`, this.buildSummary(false, {}));
  }

  finalize(completed, extra) {
    const out = `${this.logDir}/${this.sessionId}.json`;
    writeJson(out, this.buildSummary(completed, extra || {}));
    print(`Session log saved: ${out}`);
  }
}

