import {
  DEVICE_DUMP_PATH,
  LOCAL_DUMP_PATH,
  DEVICE_SCREENSHOT_PATH,
  LOCAL_SCREENSHOT_PATH,
  DEFAULT_MAX_STEPS,
  DEFAULT_STEP_DELAY,
  DEFAULT_MAX_RETRIES,
  DEFAULT_STUCK_THRESHOLD,
  DEFAULT_MAX_ELEMENTS,
  DEFAULT_LOG_DIR,
  DEFAULT_MAX_HISTORY_STEPS,
  DEFAULT_STREAMING_ENABLED,
  DEFAULT_VISION_MODE,
} from "./constants.js";

export function createConfig(overrides) {
  const baseDir = (typeof fs !== "undefined" && fs && typeof fs.cwd === "function")
    ? fs.cwd()
    : "";
  const localDumpPath = baseDir ? `${baseDir}/${LOCAL_DUMP_PATH}` : LOCAL_DUMP_PATH;
  const localScreenshotPath = baseDir ? `${baseDir}/${LOCAL_SCREENSHOT_PATH}` : LOCAL_SCREENSHOT_PATH;
  const logDir = baseDir ? `${baseDir}/${DEFAULT_LOG_DIR}` : DEFAULT_LOG_DIR;

  const cfg = {
    ADB_PATH: "adb",
    SCREEN_DUMP_PATH: DEVICE_DUMP_PATH,
    LOCAL_DUMP_PATH: localDumpPath,
    DEVICE_SCREENSHOT_PATH,
    LOCAL_SCREENSHOT_PATH: localScreenshotPath,
    MAX_STEPS: DEFAULT_MAX_STEPS,
    STEP_DELAY: DEFAULT_STEP_DELAY,
    MAX_RETRIES: DEFAULT_MAX_RETRIES,
    STUCK_THRESHOLD: DEFAULT_STUCK_THRESHOLD,
    VISION_MODE: DEFAULT_VISION_MODE,
    MAX_ELEMENTS: DEFAULT_MAX_ELEMENTS,
    LOG_DIR: logDir,
    MAX_HISTORY_STEPS: DEFAULT_MAX_HISTORY_STEPS,
    STREAMING_ENABLED: DEFAULT_STREAMING_ENABLED,
  };

  if (overrides && typeof overrides === "object") {
    for (const k of Object.keys(overrides)) {
      if (k in cfg && overrides[k] !== undefined && overrides[k] !== null) {
        cfg[k] = overrides[k];
      }
    }
  }

  if (!["off", "fallback", "always"].includes(cfg.VISION_MODE)) {
    cfg.VISION_MODE = DEFAULT_VISION_MODE;
  }

  return cfg;
}
