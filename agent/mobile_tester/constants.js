export const KEYCODE_ENTER = "66";
export const KEYCODE_HOME = "KEYCODE_HOME";
export const KEYCODE_BACK = "KEYCODE_BACK";
export const KEYCODE_DEL = "67";
export const KEYCODE_MOVE_HOME = "122";
export const KEYCODE_MOVE_END = "123";
export const KEYCODE_PASTE = "279";

export const SWIPE_COORDS = {
  up: [540, 1500, 540, 500],
  down: [540, 500, 540, 1500],
  left: [800, 1200, 200, 1200],
  right: [200, 1200, 800, 1200],
};

export function computeSwipeCoords(width, height) {
  const cx = Math.floor(width / 2);
  const cy = Math.floor(height / 2);
  const vTop = Math.floor(height * 0.208);
  const vBottom = Math.floor(height * 0.625);
  const hLeft = Math.floor(width * 0.185);
  const hRight = Math.floor(width * 0.741);
  return {
    up: [cx, vBottom, cx, vTop],
    down: [cx, vTop, cx, vBottom],
    left: [hRight, cy, hLeft, cy],
    right: [hLeft, cy, hRight, cy],
  };
}

export const SWIPE_DURATION_MS = "300";
export const LONG_PRESS_DURATION_MS = "1000";

export const DEVICE_DUMP_PATH = "/sdcard/window_dump.xml";
export const LOCAL_DUMP_PATH = "window_dump.xml";
export const DEVICE_SCREENSHOT_PATH = "/sdcard/kernel_screenshot.png";
export const LOCAL_SCREENSHOT_PATH = "kernel_screenshot.png";

export const DEFAULT_MAX_STEPS = 30;
export const DEFAULT_STEP_DELAY = 2;
export const DEFAULT_MAX_RETRIES = 3;
export const DEFAULT_STUCK_THRESHOLD = 3;
export const DEFAULT_MAX_ELEMENTS = 40;
export const DEFAULT_LOG_DIR = "logs";
export const DEFAULT_MAX_HISTORY_STEPS = 10;
export const DEFAULT_STREAMING_ENABLED = true;
export const DEFAULT_VISION_MODE = "fallback";
