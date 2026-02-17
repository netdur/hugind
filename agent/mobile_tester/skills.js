import { getInteractiveElements } from "./sanitizer.js";
import { SWIPE_DURATION_MS } from "./constants.js";
import { sleep } from "./runtime.js";

const SEND_BUTTON_PATTERN = /send|submit|post|arrow|paper.?plane/i;

function safeClipboardCommand(text) {
  const escaped = text.replaceAll("'", "'\\''");
  return ["shell", `cmd clipboard set-text '${escaped}'`];
}

export function createSkills(config, actions) {
  async function rescanScreen() {
    try {
      await actions.runAdbCommand(["shell", "uiautomator", "dump", config.SCREEN_DUMP_PATH]);
      await actions.runAdbCommand(["pull", config.SCREEN_DUMP_PATH, config.LOCAL_DUMP_PATH]);
    } catch (_err) {
      return [];
    }
    if (!fs.exists(config.LOCAL_DUMP_PATH)) return [];
    return getInteractiveElements(fs.read_text(config.LOCAL_DUMP_PATH));
  }

  async function readScreen(elements) {
    const allTexts = [];
    const seen = {};
    function collect(els) {
      let added = 0;
      for (const el of els) {
        if (el.text && !seen[el.text]) {
          seen[el.text] = true;
          allTexts.push(el.text);
          added++;
        }
      }
      return added;
    }

    collect(elements);
    const up = (actions.getSwipeCoords ? actions.getSwipeCoords() : { up: [540, 1500, 540, 500] }).up;
    let scrolls = 0;

    for (let i = 0; i < 5; i++) {
      await actions.runAdbCommand(["shell", "input", "swipe", String(up[0]), String(up[1]), String(up[2]), String(up[3]), SWIPE_DURATION_MS]);
      await sleep(1500);
      scrolls++;
      const fresh = await rescanScreen();
      if (collect(fresh) === 0) break;
    }

    const combined = allTexts.join("\n");
    if (combined) {
      await actions.runAdbCommand(safeClipboardCommand(combined));
    }

    return { success: true, message: `Read ${allTexts.length} text elements across ${scrolls} scrolls`, data: combined };
  }

  async function submitMessage(elements) {
    let candidates = elements.filter((el) => el.enabled && (el.clickable || el.action === "tap") && (SEND_BUTTON_PATTERN.test(el.text) || SEND_BUTTON_PATTERN.test(el.id)));
    if (!candidates.length) {
      const clickable = elements.filter((el) => el.enabled && el.clickable).sort((a, b) => b.center[1] - a.center[1]);
      if (clickable.length) {
        const threshold = clickable[0].center[1] * 0.8;
        candidates = clickable.filter((el) => el.center[1] >= threshold).sort((a, b) => b.center[0] - a.center[0]);
      }
    }
    if (!candidates.length) return { success: false, message: "Could not find a Send/Submit button on screen" };

    const target = candidates[0];
    await actions.runAdbCommand(["shell", "input", "tap", String(target.center[0]), String(target.center[1])]);
    await sleep(6000);

    const fresh = await rescanScreen();
    const original = {};
    for (const el of elements) if (el.text) original[el.text] = true;
    const newTexts = fresh.map((el) => el.text).filter((t) => t && !original[t]);
    if (newTexts.length) {
      return { success: true, message: `Tapped send and new content appeared: ${newTexts.slice(0, 3).join('; ')}`, data: newTexts.join("\n") };
    }
    return { success: true, message: `Tapped send at (${target.center[0]}, ${target.center[1]})` };
  }

  async function copyVisibleText(decision, elements) {
    let textElements = elements.filter((el) => el.text && el.action === "read");
    if (decision.query) {
      const q = String(decision.query).toLowerCase();
      textElements = textElements.filter((el) => el.text.toLowerCase().indexOf(q) !== -1);
    }
    if (!textElements.length) {
      textElements = elements.filter((el) => el.text);
      if (decision.query) {
        const q = String(decision.query).toLowerCase();
        textElements = textElements.filter((el) => el.text.toLowerCase().indexOf(q) !== -1);
      }
    }
    if (!textElements.length) return { success: false, message: "No readable text found on screen" };
    textElements.sort((a, b) => a.center[1] - b.center[1]);
    const combined = textElements.map((el) => el.text).join("\n");
    await actions.runAdbCommand(safeClipboardCommand(combined));
    return { success: true, message: `Copied ${textElements.length} text elements to clipboard`, data: combined };
  }

  async function waitForContent(elements) {
    const original = {};
    for (const el of elements) if (el.text) original[el.text] = true;
    for (let i = 0; i < 5; i++) {
      await sleep(3000);
      const fresh = await rescanScreen();
      const newTexts = fresh.map((el) => el.text).filter((t) => t && !original[t]);
      const totalChars = newTexts.reduce((s, t) => s + t.length, 0);
      if (totalChars > 20) {
        return { success: true, message: `New content appeared after ${(i + 1) * 3}s`, data: newTexts.slice(0, 5).join("; ") };
      }
    }
    return { success: false, message: "No new content appeared after 15s" };
  }

  function findMatch(elements, queryLower) {
    const matches = elements.filter((el) => el.text && el.text.toLowerCase().indexOf(queryLower) !== -1);
    if (!matches.length) return null;
    const scored = matches.map((el) => {
      let score = 0;
      if (el.enabled) score += 10;
      if (el.clickable || el.longClickable) score += 5;
      score += el.text.toLowerCase() === queryLower ? 20 : 5;
      return { el, score };
    }).sort((a, b) => b.score - a.score);
    return scored[0].el;
  }

  async function findAndTap(decision, elements) {
    if (!decision.query) return { success: false, message: "find_and_tap requires a query" };
    const queryLower = String(decision.query).toLowerCase();
    let best = findMatch(elements, queryLower);

    if (!best) {
      const up = (actions.getSwipeCoords ? actions.getSwipeCoords() : { up: [540, 1500, 540, 500] }).up;
      for (let i = 0; i < 10; i++) {
        await actions.runAdbCommand(["shell", "input", "swipe", String(up[0]), String(up[1]), String(up[2]), String(up[3]), SWIPE_DURATION_MS]);
        await sleep(1500);
        best = findMatch(await rescanScreen(), queryLower);
        if (best) break;
      }
    }

    if (!best) return { success: false, message: `No element matching "${decision.query}" found` };
    await actions.runAdbCommand(["shell", "input", "tap", String(best.center[0]), String(best.center[1])]);
    return { success: true, message: `Found and tapped "${best.text}"`, data: best.text };
  }

  function extractEmail(text) {
    const m = text.match(/[\w.+-]+@[\w.-]+\.\w{2,}/);
    return m ? m[0] : null;
  }

  async function composeEmail(decision, _elements) {
    let email = decision.query || null;
    const body = decision.text || "";
    if (!email && body) email = extractEmail(body);
    if (!email) return { success: false, message: "compose_email requires query=email" };

    await actions.runAdbCommand(["shell", "am", "start", "-a", "android.intent.action.SENDTO", "-d", `mailto:${email}`]);
    await sleep(2500);

    const fresh = await rescanScreen();
    const editables = fresh.filter((el) => el.editable && el.enabled).sort((a, b) => a.center[1] - b.center[1]);
    if (!editables.length) return { success: false, message: "Launched email compose but no editable fields appeared" };

    const bodyField = editables[editables.length - 1];
    await actions.runAdbCommand(["shell", "input", "tap", String(bodyField.center[0]), String(bodyField.center[1])]);
    await sleep(300);

    if (body) {
      await actions.runAdbCommand(safeClipboardCommand(body));
      await sleep(200);
    }
    await actions.runAdbCommand(["shell", "input", "keyevent", "279"]);
    return { success: true, message: `Email compose opened to ${email}, body pasted` };
  }

  async function executeSkill(decision, elements) {
    const skill = decision.skill || decision.action;
    switch (skill) {
      case "read_screen": return readScreen(elements);
      case "submit_message": return submitMessage(elements);
      case "copy_visible_text": return copyVisibleText(decision, elements);
      case "wait_for_content": return waitForContent(elements);
      case "find_and_tap": return findAndTap(decision, elements);
      case "compose_email": return composeEmail(decision, elements);
      default: return { success: false, message: `Unknown skill: ${skill}` };
    }
  }

  return { executeSkill };
}
