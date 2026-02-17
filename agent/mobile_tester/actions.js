import {
  KEYCODE_ENTER,
  KEYCODE_HOME,
  KEYCODE_BACK,
  KEYCODE_DEL,
  KEYCODE_MOVE_HOME,
  KEYCODE_MOVE_END,
  KEYCODE_PASTE,
  SWIPE_COORDS,
  SWIPE_DURATION_MS,
  LONG_PRESS_DURATION_MS,
  DEVICE_SCREENSHOT_PATH,
  LOCAL_SCREENSHOT_PATH,
  computeSwipeCoords,
} from "./constants.js";
import { sleep, log } from "./runtime.js";

let dynamicSwipeCoords = null;

export function sanitizeCoordinates(raw) {
  if (raw == null) return undefined;
  if (Array.isArray(raw) && raw.length >= 2) {
    const x = Number(raw[0]);
    const y = Number(raw[1]);
    if (Number.isFinite(x) && Number.isFinite(y) && x <= 10000 && y <= 10000) {
      return [Math.round(x), Math.round(y)];
    }
  }
  if (Array.isArray(raw) && raw.length === 1) {
    const split = trySplitConcatenated(Number(raw[0]));
    if (split) return split;
  }
  if (typeof raw === "number" && raw > 10000) {
    return trySplitConcatenated(raw) || undefined;
  }
  if (typeof raw === "string") {
    const parts = raw.split(/[,\s]+/).map(Number);
    if (parts.length >= 2 && parts.every(Number.isFinite)) {
      return [Math.round(parts[0]), Math.round(parts[1])];
    }
  }
  return undefined;
}

function trySplitConcatenated(n) {
  if (!Number.isFinite(n) || n <= 0) return null;
  const s = String(Math.round(n));
  for (let i = 2; i <= Math.min(4, s.length - 2); i++) {
    const x = Number(s.slice(0, i));
    const y = Number(s.slice(i));
    if (x > 0 && x <= 3000 && y > 0 && y <= 5000) return [x, y];
  }
  return null;
}

function validateCoordinates(coords) {
  if (!coords || !Array.isArray(coords) || coords.length < 2) return null;
  const x = coords[0];
  const y = coords[1];
  if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
  if (x < 0 || y < 0 || x > 10000 || y > 10000) return null;
  return [Math.round(x), Math.round(y)];
}

export function initDeviceContext(resolution) {
  dynamicSwipeCoords = computeSwipeCoords(resolution[0], resolution[1]);
}

export function getSwipeCoords() {
  return dynamicSwipeCoords || SWIPE_COORDS;
}

export function createActions(config) {
  async function runAdbCommand(command, retries) {
    const maxRetries = retries == null ? config.MAX_RETRIES : retries;
    for (let attempt = 0; attempt <= maxRetries; attempt++) {
      const stdout = await spawn(config.ADB_PATH, command);
      if (String(stdout).toLowerCase().indexOf("error:") !== -1) {
        if (attempt < maxRetries) {
          const delay = Math.pow(2, attempt) * 1000;
          log(`ADB Error (attempt ${attempt + 1}/${maxRetries + 1}): ${stdout.trim()}`);
          await sleep(delay);
          continue;
        }
      }
      return String(stdout || "").trim();
    }
    return "";
  }

  async function getScreenResolution() {
    try {
      const output = await runAdbCommand(["shell", "wm", "size"]);
      let m = output.match(/Override size:\s*(\d+)x(\d+)/);
      if (!m) m = output.match(/Physical size:\s*(\d+)x(\d+)/);
      if (m) return [Number(m[1]), Number(m[2])];
    } catch (_err) {}
    return null;
  }

  async function getForegroundApp() {
    try {
      const output = await runAdbCommand(["shell", "dumpsys", "activity", "activities"]);
      const m = output.match(/mResumedActivity.*?(\S+\/\S+)/);
      if (m) return m[1].replace("}", "");
    } catch (_err) {}
    try {
      const output = await runAdbCommand(["shell", "dumpsys", "window", "windows"]);
      const mCurrent = output.match(/mCurrentFocus.*?(\S+\/\S+)/);
      if (mCurrent) return mCurrent[1].replace("}", "");
      const mFocused = output.match(/mFocusedApp.*?(\S+\/\S+)/);
      if (mFocused) return mFocused[1].replace("}", "");
    } catch (_err2) {}
    return null;
  }

  async function executeTap(action) {
    const coords = validateCoordinates(action.coordinates);
    if (!coords) return { success: false, message: `Invalid coordinates: ${JSON.stringify(action.coordinates)}` };
    await runAdbCommand(["shell", "input", "tap", String(coords[0]), String(coords[1])]);
    return { success: true, message: `Tapped (${coords[0]}, ${coords[1]})` };
  }

  async function executeType(action) {
    const text = action.text || "";
    if (!text) return { success: false, message: "No text to type" };
    if (action.coordinates) {
      const coords = validateCoordinates(action.coordinates);
      if (coords) {
        await runAdbCommand(["shell", "input", "tap", String(coords[0]), String(coords[1])]);
        await sleep(300);
      }
    }
    const escaped = text
      .replaceAll("\\", "\\\\")
      .replaceAll("\"", "\\\"")
      .replaceAll("'", "\\'")
      .replaceAll("`", "\\`")
      .replaceAll("$", "\\$")
      .replaceAll("!", "\\!")
      .replaceAll("?", "\\?")
      .replaceAll(" ", "%s")
      .replaceAll("&", "\\&")
      .replaceAll("|", "\\|")
      .replaceAll(";", "\\;")
      .replaceAll("(", "\\(")
      .replaceAll(")", "\\)")
      .replaceAll("[", "\\[")
      .replaceAll("]", "\\]")
      .replaceAll("{", "\\{")
      .replaceAll("}", "\\}")
      .replaceAll("<", "\\<")
      .replaceAll(">", "\\>");
    await runAdbCommand(["shell", "input", "text", escaped]);
    return { success: true, message: `Typed "${text}"` };
  }

  async function executeSwipe(action) {
    const direction = action.direction || "up";
    const coords = (getSwipeCoords()[direction] || getSwipeCoords().up);
    await runAdbCommand([
      "shell", "input", "swipe",
      String(coords[0]), String(coords[1]), String(coords[2]), String(coords[3]), SWIPE_DURATION_MS,
    ]);
    return { success: true, message: `Swiped ${direction}` };
  }

  async function executeLongPress(action) {
    const coords = validateCoordinates(action.coordinates);
    if (!coords) return { success: false, message: `Invalid coordinates: ${JSON.stringify(action.coordinates)}` };
    await runAdbCommand([
      "shell", "input", "swipe",
      String(coords[0]), String(coords[1]), String(coords[0]), String(coords[1]), LONG_PRESS_DURATION_MS,
    ]);
    return { success: true, message: `Long pressed (${coords[0]}, ${coords[1]})` };
  }

  async function executeScreenshot(action) {
    const filename = action.filename || LOCAL_SCREENSHOT_PATH;
    await runAdbCommand(["shell", "screencap", "-p", DEVICE_SCREENSHOT_PATH]);
    await runAdbCommand(["pull", DEVICE_SCREENSHOT_PATH, filename]);
    return { success: true, message: `Screenshot saved to ${filename}`, data: filename };
  }

  async function executeLaunch(action) {
    if (!action.package && !action.uri && !action.activity) {
      return {
        success: false,
        message: "launch requires at least one of: package, uri, or package+activity",
      };
    }

    if (action.package && !action.activity && !action.uri) {
      const result = await runAdbCommand(["shell", "monkey", "-p", action.package, "-c", "android.intent.category.LAUNCHER", "1"]);
      return { success: true, message: `Launched ${action.package}`, data: result };
    }

    const args = ["shell", "am", "start"];
    if (action.uri) {
      args.push("-a", "android.intent.action.VIEW");
      args.push("-d", action.uri);
    }
    if (action.package && action.activity) {
      args.push("-n", `${action.package}/${action.activity}`);
    }
    if (!action.uri && !(action.package && action.activity)) {
      return {
        success: false,
        message: "launch with activity requires both package and activity, or provide uri",
      };
    }
    if (action.extras) {
      for (const k of Object.keys(action.extras)) {
        args.push("--es", k, String(action.extras[k]));
      }
    }

    const label = action.package || action.uri || "intent";
    const result = await runAdbCommand(args);
    return { success: true, message: `Launched ${label}`, data: result };
  }

  async function executeClear() {
    await runAdbCommand(["shell", "input", "keyevent", KEYCODE_MOVE_END]);
    await runAdbCommand(["shell", "input", "keyevent", "--longpress", KEYCODE_MOVE_HOME]);
    await runAdbCommand(["shell", "input", "keyevent", KEYCODE_DEL]);
    return { success: true, message: "Cleared text field" };
  }

  async function executeClipboardGet() {
    const result = await runAdbCommand(["shell", "cmd", "clipboard", "get-text"]);
    if (result) return { success: true, message: `Clipboard: ${result}`, data: result };
    const fallback = await runAdbCommand(["shell", "service", "call", "clipboard", "2", "i32", "1"]);
    return { success: true, message: `Clipboard (raw): ${fallback}`, data: fallback };
  }

  async function executeClipboardSet(action) {
    const text = action.text || "";
    if (!text) return { success: false, message: "No text to set on clipboard" };
    const escaped = text.replaceAll("'", "'\\''");
    await runAdbCommand(["shell", `cmd clipboard set-text '${escaped}'`]);
    return { success: true, message: `Clipboard set to "${text.slice(0, 50)}"` };
  }

  async function executePaste(action) {
    if (action.coordinates) {
      const coords = validateCoordinates(action.coordinates);
      if (coords) {
        await runAdbCommand(["shell", "input", "tap", String(coords[0]), String(coords[1])]);
        await sleep(300);
      }
    }
    await runAdbCommand(["shell", "input", "keyevent", KEYCODE_PASTE]);
    return { success: true, message: "Pasted clipboard content" };
  }

  const SCROLL_TO_SWIPE = { down: "up", up: "down", left: "right", right: "left" };
  async function executeScroll(action) {
    const direction = action.direction || "down";
    const swipeDir = SCROLL_TO_SWIPE[direction] || "up";
    return executeSwipe({ action: "swipe", direction: swipeDir }).then(() => ({ success: true, message: `Scrolled ${direction}` }));
  }

  async function executeNotifications() {
    const raw = await runAdbCommand(["shell", "dumpsys", "notification", "--noredact"]);
    const notifications = [];
    let currentTitle = "";
    for (const line of raw.split("\n")) {
      const titleMatch = line.match(/android\.title=(?:String\s*\()?(.*?)(?:\)|$)/);
      const textMatch = line.match(/android\.text=(?:String\s*\()?(.*?)(?:\)|$)/);
      if (titleMatch) currentTitle = titleMatch[1].trim();
      if (textMatch && currentTitle) {
        notifications.push(`${currentTitle}: ${textMatch[1].trim()}`);
        currentTitle = "";
      }
    }
    const summary = notifications.length ? notifications.join("\n") : "No notifications found";
    return { success: true, message: `Notifications:\n${summary}`, data: summary };
  }

  async function executeOpenSettings(action) {
    const map = {
      wifi: "android.settings.WIFI_SETTINGS",
      bluetooth: "android.settings.BLUETOOTH_SETTINGS",
      display: "android.settings.DISPLAY_SETTINGS",
      sound: "android.settings.SOUND_SETTINGS",
      battery: "android.settings.BATTERY_SAVER_SETTINGS",
      location: "android.settings.LOCATION_SOURCE_SETTINGS",
      apps: "android.settings.APPLICATION_SETTINGS",
      date: "android.settings.DATE_SETTINGS",
      accessibility: "android.settings.ACCESSIBILITY_SETTINGS",
      developer: "android.settings.APPLICATION_DEVELOPMENT_SETTINGS",
    };
    const setting = action.setting || "";
    const intent = map[setting];
    if (!intent) return { success: false, message: `Unknown setting \"${setting}\"` };
    const result = await runAdbCommand(["shell", "am", "start", "-a", intent]);
    return { success: true, message: `Opened ${setting} settings`, data: result };
  }

  async function executeShell(action) {
    const cmd = action.command || "";
    if (!cmd) return { success: false, message: "No command provided" };
    const result = await runAdbCommand(["shell", ...cmd.split(" ")]);
    return { success: true, message: `Shell output: ${result.slice(0, 200)}`, data: result };
  }

  async function executeAction(action) {
    switch (action.action) {
      case "tap": return executeTap(action);
      case "type": return executeType(action);
      case "enter": await runAdbCommand(["shell", "input", "keyevent", KEYCODE_ENTER]); return { success: true, message: "Pressed Enter" };
      case "swipe": return executeSwipe(action);
      case "home": await runAdbCommand(["shell", "input", "keyevent", KEYCODE_HOME]); return { success: true, message: "Went home" };
      case "back": await runAdbCommand(["shell", "input", "keyevent", KEYCODE_BACK]); return { success: true, message: "Went back" };
      case "wait": await sleep(2000); return { success: true, message: "Waited 2s" };
      case "done": return { success: true, message: "done" };
      case "longpress": return executeLongPress(action);
      case "screenshot": return executeScreenshot(action);
      case "launch": return executeLaunch(action);
      case "clear": return executeClear();
      case "clipboard_get": return executeClipboardGet();
      case "clipboard_set": return executeClipboardSet(action);
      case "paste": return executePaste(action);
      case "shell": return executeShell(action);
      case "scroll": return executeScroll(action);
      case "open_url":
        if (!action.url) return { success: false, message: "No URL provided" };
        return runAdbCommand(["shell", "am", "start", "-a", "android.intent.action.VIEW", "-d", action.url]).then((data) => ({ success: true, message: `Opened URL: ${action.url}`, data }));
      case "switch_app":
        if (!action.package) return { success: false, message: "No package name provided" };
        return runAdbCommand(["shell", "monkey", "-p", action.package, "-c", "android.intent.category.LAUNCHER", "1"]).then((data) => ({ success: true, message: `Switched to ${action.package}`, data }));
      case "notifications": return executeNotifications();
      case "pull_file": {
        const devicePath = action.path || "";
        if (!devicePath) return { success: false, message: "No device path provided" };
        if (!fs.exists("./pulled_files")) fs.mkdir("./pulled_files", true);
        const filename = devicePath.split("/").pop() || "file";
        const localPath = `./pulled_files/${filename}`;
        const data = await runAdbCommand(["pull", devicePath, localPath]);
        return { success: true, message: `Pulled ${devicePath} -> ${localPath}`, data };
      }
      case "push_file": {
        const source = action.source || "";
        const dest = action.dest || "";
        if (!source || !dest) return { success: false, message: "Missing source or dest path" };
        const data = await runAdbCommand(["push", source, dest]);
        return { success: true, message: `Pushed ${source} -> ${dest}`, data };
      }
      case "keyevent":
        if (action.code == null) return { success: false, message: "No keycode provided" };
        await runAdbCommand(["shell", "input", "keyevent", String(action.code)]);
        return { success: true, message: `Sent keyevent ${action.code}` };
      case "open_settings": return executeOpenSettings(action);
      default: return { success: false, message: `Unknown action: ${action.action}` };
    }
  }

  return {
    runAdbCommand,
    getScreenResolution,
    getForegroundApp,
    getSwipeCoords,
    executeAction,
  };
}
