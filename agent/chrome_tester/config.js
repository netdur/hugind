import {
  DEFAULT_MAX_STEPS,
  DEFAULT_STEP_DELAY,
  DEFAULT_STUCK_THRESHOLD,
  DEFAULT_MAX_HISTORY_STEPS,
  DEFAULT_MAX_STATE_CHARS,
  DEFAULT_LOG_DIR,
} from "./constants.js";

export function createConfig(overrides) {
  const baseDir = (typeof fs !== "undefined" && fs && typeof fs.cwd === "function")
    ? fs.cwd()
    : "";
  const logDir = baseDir ? `${baseDir}/${DEFAULT_LOG_DIR}` : DEFAULT_LOG_DIR;

  const cfg = {
    MAX_STEPS: DEFAULT_MAX_STEPS,
    STEP_DELAY: DEFAULT_STEP_DELAY,
    STUCK_THRESHOLD: DEFAULT_STUCK_THRESHOLD,
    MAX_HISTORY_STEPS: DEFAULT_MAX_HISTORY_STEPS,
    MAX_STATE_CHARS: DEFAULT_MAX_STATE_CHARS,
    LOG_DIR: logDir,
  };

  if (overrides && typeof overrides === "object") {
    for (const k of Object.keys(overrides)) {
      if (k in cfg && overrides[k] !== undefined && overrides[k] !== null) {
        cfg[k] = overrides[k];
      }
    }
  }

  return cfg;
}

