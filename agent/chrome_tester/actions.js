

const KNOWN_CHROME_TOOLS = {
  take_snapshot: true,
  take_screenshot: true,
  navigate_page: true,
  navigate: true,
  new_page: true,
  list_pages: true,
  select_page: true,
  click: true,
  fill: true,
  fill_form: true,
  type_text: true,
  press_key: true,
  wait_for: true,
  evaluate_script: true,
  evaluate: true,
};

const OVERLAY_DETECTION_SCRIPT = `() => {
  const adRe = /(ad|ads|doubleclick|googlesyndication|taboola|outbrain|adnxs|amazon-adsystem|criteo|pubmatic)/i;
  const vw = Math.max(1, window.innerWidth || 0);
  const vh = Math.max(1, window.innerHeight || 0);
  const area = vw * vh;
  const nodes = Array.from(document.querySelectorAll("body *"));
  const candidates = [];

  for (const el of nodes) {
    const style = window.getComputedStyle(el);
    if (!style) continue;
    if (style.display === "none" || style.visibility === "hidden" || style.opacity === "0") continue;
    if (!(style.position === "fixed" || style.position === "sticky")) continue;
    if (style.pointerEvents === "none") continue;

    const rect = el.getBoundingClientRect();
    if (!rect || rect.width <= 0 || rect.height <= 0) continue;
    const cover = (Math.max(0, rect.width) * Math.max(0, rect.height)) / area;
    if (cover < 0.2) continue;

    const z = Number.parseInt(style.zIndex || "0", 10);
    if (Number.isFinite(z) && z < 999) continue;

    const attrs = String((el.id || "") + " " + (el.className || "") + " " + (el.getAttribute("name") || ""));
    const hasAdSignal = adRe.test(attrs);
    candidates.push({
      element: el,
      tag: el.tagName,
      id: el.id || "",
      className: typeof el.className === "string" ? el.className : "",
      cover,
      zIndex: Number.isFinite(z) ? z : null,
      hasAdSignal
    });
  }

  candidates.sort((a, b) => (b.cover - a.cover) || ((b.zIndex || 0) - (a.zIndex || 0)));
  const centerEl = document.elementFromPoint(Math.floor(vw / 2), Math.floor(vh / 2));
  const centerCovered = centerEl
    ? candidates.some((c) => c.element === centerEl || c.element.contains(centerEl))
    : false;

  const bodyStyle = window.getComputedStyle(document.body || document.documentElement);
  const bodyLocked = bodyStyle ? bodyStyle.overflow === "hidden" : false;

  const top = candidates[0] || null;
  const blocked = !!top && (top.cover >= 0.3 || centerCovered || bodyLocked);

  return {
    blocked,
    centerCovered,
    bodyLocked,
    candidateCount: candidates.length,
    top: top ? {
      tag: top.tag,
      id: top.id,
      className: top.className,
      cover: Number(top.cover.toFixed(3)),
      zIndex: top.zIndex,
      hasAdSignal: top.hasAdSignal
    } : null
  };
}`;

const OVERLAY_DISMISS_SCRIPT = `() => {
  const adRe = /(ad|ads|doubleclick|googlesyndication|taboola|outbrain|adnxs|amazon-adsystem|criteo|pubmatic)/i;
  const closeRe = /(close|dismiss|skip|not now|x)/i;
  const vw = Math.max(1, window.innerWidth || 0);
  const vh = Math.max(1, window.innerHeight || 0);
  const area = vw * vh;
  const nodes = Array.from(document.querySelectorAll("body *"));
  const candidates = [];

  for (const el of nodes) {
    const style = window.getComputedStyle(el);
    if (!style) continue;
    if (style.display === "none" || style.visibility === "hidden" || style.opacity === "0") continue;
    if (!(style.position === "fixed" || style.position === "sticky")) continue;
    if (style.pointerEvents === "none") continue;

    const rect = el.getBoundingClientRect();
    if (!rect || rect.width <= 0 || rect.height <= 0) continue;
    const cover = (Math.max(0, rect.width) * Math.max(0, rect.height)) / area;
    if (cover < 0.2) continue;

    const z = Number.parseInt(style.zIndex || "0", 10);
    if (Number.isFinite(z) && z < 999) continue;

    const attrs = String((el.id || "") + " " + (el.className || "") + " " + (el.getAttribute("name") || ""));
    const hasAdSignal = adRe.test(attrs);
    candidates.push({ element: el, cover, zIndex: Number.isFinite(z) ? z : null, hasAdSignal });
  }

  candidates.sort((a, b) => (b.cover - a.cover) || ((b.zIndex || 0) - (a.zIndex || 0)));
  let clickedClose = 0;
  let hidden = 0;

  for (const c of candidates.slice(0, 3)) {
    const controls = [c.element, ...Array.from(c.element.querySelectorAll("button,[role='button'],a,[aria-label],[id],[class]"))];
    let closed = false;
    for (const ctrl of controls) {
      const cs = window.getComputedStyle(ctrl);
      if (!cs || cs.display === "none" || cs.visibility === "hidden") continue;
      const label = String((ctrl.getAttribute("aria-label") || "") + " " + (ctrl.textContent || "") + " " + (ctrl.id || "") + " " + (ctrl.className || "")).trim();
      if (!label || !closeRe.test(label)) continue;
      try {
        ctrl.click();
        clickedClose++;
        closed = true;
        break;
      } catch (_e) {}
    }
    if (!closed) {
      try {
        c.element.style.setProperty("display", "none", "important");
        c.element.style.setProperty("visibility", "hidden", "important");
        c.element.style.setProperty("pointer-events", "none", "important");
        hidden++;
      } catch (_e2) {}
    }
  }

  return {
    dismissed: clickedClose + hidden > 0,
    clickedClose,
    hidden,
    candidateCount: candidates.length
  };
}`;

const PHONE_EXTRACTION_SCRIPT = `() => {
  const out = [];
  const seen = new Set();
  const phoneRe = /(?:\\+?\\d[\\d\\s().-]{6,}\\d)/g;

  function normalize(raw) {
    const s = String(raw || "").trim();
    if (!s) return "";
    const compact = s.replace(/\\s+/g, " ").trim();
    const digits = compact.replace(/[^\\d+]/g, "");
    if (digits.replace(/\\D/g, "").length < 7) return "";
    return compact;
  }

  function push(raw, source) {
    const n = normalize(raw);
    if (!n) return;
    const key = n.replace(/[\\s().-]/g, "");
    if (seen.has(key)) return;
    seen.add(key);
    out.push({ phone: n, source });
  }

  const bodyText = (document.body && document.body.innerText) ? document.body.innerText : "";
  const matches = bodyText.match(phoneRe) || [];
  for (const m of matches) push(m, "body_text");

  for (const a of Array.from(document.querySelectorAll('a[href^="tel:"]'))) {
    push(a.getAttribute("href") || "", "tel_link");
    push(a.textContent || "", "tel_link_text");
  }

  for (const el of Array.from(document.querySelectorAll('[aria-label], [data-item-id], [data-value]'))) {
    const label = (el.getAttribute("aria-label") || "") + " " + (el.getAttribute("data-item-id") || "") + " " + (el.getAttribute("data-value") || "");
    const m = label.match(phoneRe) || [];
    for (const x of m) push(x, "attributes");
  }

  return {
    count: out.length,
    numbers: out.slice(0, 8)
  };
}`;

function safeJsonParse(raw) {
  try {
    return JSON.parse(raw);
  } catch (_err) {
    return null;
  }
}

function extractBaseToolName(name) {
  const s = String(name || "");
  const idx = s.indexOf(":");
  return idx === -1 ? s : s.slice(idx + 1);
}

function isChromeServerName(server) {
  return String(server || "").toLowerCase().indexOf("chrome") !== -1;
}

function normalizeTool(tool) {
  const name = String((tool && tool.name) || "");
  const server = String((tool && tool.server) || "");
  return {
    name,
    server,
    base: extractBaseToolName(name),
    description: String((tool && tool.description) || ""),
  };
}

function selectChromeTools(allTools) {
  if (!allTools.length) return [];

  const byServer = allTools.filter((t) => isChromeServerName(t.server) || isChromeServerName(t.name.split(":")[0]));
  if (byServer.length) return byServer;

  const byKnownNames = allTools.filter((t) => KNOWN_CHROME_TOOLS[t.base]);
  if (byKnownNames.length) return byKnownNames;

  const uniqueServers = {};
  for (const t of allTools) {
    if (t.server) uniqueServers[t.server] = true;
  }
  if (Object.keys(uniqueServers).length === 1) return allTools;

  return [];
}

function resolveCallName(tool) {
  if (!tool) return "";
  if (tool.name.indexOf(":") !== -1) return tool.name;
  if (tool.server) return `${tool.server}:${tool.name}`;
  return tool.name;
}

function resolveCapability(tools, candidates) {
  for (const c of candidates) {
    const exact = tools.find((t) => t.base === c);
    if (exact) return { ...exact, callNames: [resolveCallName(exact)] };
  }
  return null;
}

function buildFallbackCallNames(candidates) {
  const prefixes = ["chrome-devtools", "chrome", ""];
  const out = [];
  const seen = {};
  for (const c of candidates) {
    for (const p of prefixes) {
      const name = p ? `${p}:${c}` : c;
      if (!seen[name]) {
        seen[name] = true;
        out.push(name);
      }
    }
  }
  return out;
}

function summarizeToolResult(value) {
  if (typeof value === "string") return value;

  if (value && typeof value === "object") {
    if (Array.isArray(value.content)) {
      const textParts = value.content
        .filter((x) => x && typeof x === "object" && x.type === "text" && typeof x.text === "string")
        .map((x) => x.text);
      if (textParts.length) return textParts.join("\n");
    }

    for (const k of ["snapshot", "text", "message", "result", "value"]) {
      if (typeof value[k] === "string" && value[k]) return value[k];
    }
  }

  return JSON.stringify(value, null, 2);
}

function hashText(s) {
  const text = String(s || "");
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h += (h << 1) + (h << 4) + (h << 7) + (h << 8) + (h << 24);
  }
  return String(h >>> 0);
}

function extractJsonFromText(text) {
  const s = String(text || "").trim();
  if (!s) return null;
  const direct = safeJsonParse(s);
  if (direct && typeof direct === "object") return direct;
  // Try to extract JSON object {...} or array [...]
  const mObj = s.match(/\{[\s\S]*\}/);
  if (mObj) {
    const parsed = safeJsonParse(mObj[0]);
    if (parsed && typeof parsed === "object") return parsed;
  }
  const mArr = s.match(/\[[\s\S]*\]/);
  if (mArr) {
    const parsed = safeJsonParse(mArr[0]);
    if (Array.isArray(parsed)) return parsed;
  }
  return null;
}

function extractAnyObject(value) {
  if (value == null) return null;
  if (typeof value === "string") return extractJsonFromText(value);
  if (Array.isArray(value)) {
    for (const item of value) {
      const found = extractAnyObject(item);
      if (found) return found;
    }
    return null;
  }
  if (typeof value !== "object") return null;

  if (typeof value.blocked === "boolean") return value;
  if (typeof value.dismissed === "boolean") return value;
  if (typeof value.clickedClose === "number" || typeof value.hidden === "number") return value;
  if (value.value && typeof value.value === "object") {
    const found = extractAnyObject(value.value);
    if (found) return found;
  }
  if (value.result && typeof value.result === "object") {
    const found = extractAnyObject(value.result);
    if (found) return found;
  }
  if (value.data && typeof value.data === "object") {
    const found = extractAnyObject(value.data);
    if (found) return found;
  }
  if (Array.isArray(value.content)) {
    for (const item of value.content) {
      if (item && typeof item === "object" && typeof item.text === "string") {
        const parsed = extractJsonFromText(item.text);
        if (parsed) return parsed;
      }
    }
  }
  return value;
}

// --- Structured snapshot processing (OpenClaw-style) ---

const INTERACTIVE_ROLES = new Set([
  "link", "button", "searchbox", "textbox", "input", "combobox",
  "checkbox", "radio", "menuitem", "menuitemcheckbox", "menuitemradio",
  "tab", "switch", "slider", "spinbutton", "option", "treeitem",
]);

const CONTENT_ROLES = new Set([
  "heading", "image", "img", "paragraph", "blockquote", "caption",
  "cell", "columnheader", "rowheader", "status", "alert",
]);

const LANDMARK_ROLES = new Set([
  "banner", "navigation", "main", "contentinfo", "complementary",
  "form", "region", "search",
]);

function escapeQuoted(v) {
  return String(v || "").replace(/\\/g, "\\\\").replace(/"/g, '\\"');
}

// Extract structured snapshot tree from MCP response value
function extractSnapshotTree(value) {
  if (!value) return null;
  // Direct tree object with role/children
  if (value.role && (value.children || value.id)) return value;
  // Wrapped in snapshot key
  if (value.snapshot && typeof value.snapshot === "object") return value.snapshot;
  // In content array (structured content format)
  if (Array.isArray(value.content)) {
    for (const item of value.content) {
      if (!item) continue;
      // Structured content item with data
      if (item.type === "resource" && item.resource && item.resource.text) {
        const parsed = safeJsonParse(item.resource.text);
        if (parsed) { const t = extractSnapshotTree(parsed); if (t) return t; }
      }
      if (item.type === "text" && typeof item.text === "string") {
        const parsed = safeJsonParse(item.text);
        if (parsed) { const t = extractSnapshotTree(parsed); if (t) return t; }
      }
      if (typeof item === "object" && item.role) return item;
    }
  }
  // In data/value/result/structuredContent wrappers
  for (const k of ["structuredContent", "data", "value", "result"]) {
    if (value[k] && typeof value[k] === "object") {
      const t = extractSnapshotTree(value[k]);
      if (t) return t;
    }
  }
  return null;
}

// Build full indented text from structured tree (with all nodes)
function buildFullText(tree) {
  const lines = [];
  function visit(node, depth) {
    if (!node) return;
    const role = String(node.role || "generic").toLowerCase().trim();
    const name = node.name ? String(node.name).trim() : "";
    const uid = node.id ? String(node.id).trim() : "";
    const value = node.value != null ? String(node.value) : "";
    const desc = node.description ? String(node.description).trim() : "";

    const indent = "  ".repeat(depth);
    let line = indent;
    if (uid) line += `uid=${uid} `;
    line += role;
    if (name) line += ` "${escapeQuoted(name)}"`;
    if (value) line += ` value="${escapeQuoted(value)}"`;
    if (desc) line += ` description="${escapeQuoted(desc)}"`;

    lines.push(line);
    if (Array.isArray(node.children)) {
      for (const child of node.children) visit(child, depth + 1);
    }
  }
  visit(tree, 0);
  return lines.join("\n");
}

// Build compact summary: only interactive + content + landmark roles
function buildCompactSummary(tree) {
  const lines = [];
  let totalNodes = 0;
  let interactiveCount = 0;

  function visit(node, depth) {
    if (!node) return;
    totalNodes++;
    const role = String(node.role || "generic").toLowerCase().trim();
    const name = node.name ? String(node.name).trim() : "";
    const uid = node.id ? String(node.id).trim() : "";
    const value = node.value != null ? String(node.value) : "";
    const desc = node.description ? String(node.description).trim() : "";

    const isInteractive = INTERACTIVE_ROLES.has(role);
    const isContent = CONTENT_ROLES.has(role);
    const isLandmark = LANDMARK_ROLES.has(role);
    const isRoot = role === "rootwebarea" || role === "webarea";
    const isIframe = role === "iframe";

    if (isInteractive) interactiveCount++;

    // Include: interactive, content with name, landmarks, root, iframes
    if (isInteractive || (isContent && name) || isLandmark || isRoot || isIframe) {
      const indent = "  ".repeat(depth);
      let line = indent;
      if (uid) line += `uid=${uid} `;
      line += role;
      if (name) line += ` "${escapeQuoted(name)}"`;
      if (value) line += ` value="${escapeQuoted(value)}"`;
      if (desc) line += ` description="${escapeQuoted(desc)}"`;
      lines.push(line);
    }

    if (Array.isArray(node.children)) {
      for (const child of node.children) visit(child, depth + 1);
    }
  }
  visit(tree, 0);

  const header = `PAGE_SUMMARY (${interactiveCount} interactive, ${totalNodes} total nodes):`;
  return header + "\n" + lines.join("\n");
}

const ICON_HINTS_SCRIPT = [
  "() => {",
  "  var hints = [];",
  "  var els = document.querySelectorAll('a, button, [role=button]');",
  "  for (var i = 0; i < els.length; i++) {",
  "    var el = els[i];",
  "    var text = (el.textContent || '').trim();",
  "    if (text.length > 2) continue;",
  "    var cls = el.getAttribute('class') || '';",
  "    var children = el.querySelectorAll('*');",
  "    for (var j = 0; j < children.length; j++) {",
  "      var cc = children[j].getAttribute('class') || '';",
  "      if (cc) cls = cls + ' ' + cc;",
  "    }",
  "    cls = cls.trim();",
  "    if (!cls) continue;",
  "    var words = cls.replace(/[^a-zA-Z]+/g, ' ').trim();",
  "    if (!words) continue;",
  "    hints.push({",
  "      tag: el.tagName.toLowerCase(),",
  "      href: el.getAttribute('href') || '',",
  "      aria: el.getAttribute('aria-label') || '',",
  "      cls: words,",
  "      text: text",
  "    });",
  "  }",
  "  return hints.slice(0, 20);",
  "}",
].join("\n");

export function createActions(config) {
  let capabilities = null;
  let lastFullSnapshot = "";  // text form, stored for filter_snapshot
  let lastSnapshotTree = null; // structured tree if available

  async function callTool(toolName, args) {
    const raw = await tools.call(toolName, args || {});
    const parsed = safeJsonParse(raw);
    const value = parsed == null ? raw : parsed;
    return {
      raw,
      value,
      summary: summarizeToolResult(value),
    };
  }

  async function callWithVariants(capability, variants) {
    if (!capability) return { success: false, message: "Capability unavailable" };
    const tries = Array.isArray(variants) ? variants : [variants || {}];
    const toolNames = Array.isArray(capability.callNames) && capability.callNames.length
      ? capability.callNames
      : (capability.callName ? [capability.callName] : []);
    if (!toolNames.length) return { success: false, message: "Capability has no callable tool name" };
    let lastErr = "";
    for (const toolName of toolNames) {
      for (const args of tries) {
        try {
          const out = await callTool(toolName, args || {});
          return {
            success: true,
            message: `Called ${toolName}`,
            data: out.value,
            summary: out.summary,
            args: args || {},
            tool: toolName,
          };
        } catch (err) {
          lastErr = String(err && err.message ? err.message : err);
        }
      }
    }
    return {
      success: false,
      message: `All tool/argument variants failed: ${lastErr}`,
    };
  }

  async function discoverCapabilities() {
    let listRaw = "";
    let list = [];
    let listErr = "";
    try {
      listRaw = await tools.list();
      const parsed = safeJsonParse(listRaw);
      if (Array.isArray(parsed)) list = parsed;
    } catch (err) {
      listErr = String(err && err.message ? err.message : err);
    }

    let chromeTools = [];
    if (list.length) {
      const normalized = list.map(normalizeTool).filter((t) => t.name);
      chromeTools = selectChromeTools(normalized);
      if (!chromeTools.length) {
        return { success: false, message: "No Chrome MCP tools found from tools.list()" };
      }
    }

    const fromDiscovery = list.length > 0;
    const caps = fromDiscovery ? {
      navigate: resolveCapability(chromeTools, ["navigate_page", "navigate"]),
      newPage: resolveCapability(chromeTools, ["new_page"]),
      snapshot: resolveCapability(chromeTools, ["take_snapshot", "snapshot"]),
      click: resolveCapability(chromeTools, ["click"]),
      fill: resolveCapability(chromeTools, ["fill", "fill_form"]),
      typeText: resolveCapability(chromeTools, ["type_text"]),
      pressKey: resolveCapability(chromeTools, ["press_key"]),
      waitFor: resolveCapability(chromeTools, ["wait_for"]),
      listPages: resolveCapability(chromeTools, ["list_pages"]),
      selectPage: resolveCapability(chromeTools, ["select_page"]),
      evaluate: resolveCapability(chromeTools, ["evaluate_script", "evaluate"]),
      screenshot: resolveCapability(chromeTools, ["take_screenshot", "screenshot"]),
      allChromeTools: chromeTools,
    } : {
      navigate: { callNames: buildFallbackCallNames(["navigate_page", "navigate"]) },
      newPage: { callNames: buildFallbackCallNames(["new_page"]) },
      snapshot: { callNames: buildFallbackCallNames(["take_snapshot", "snapshot"]) },
      click: { callNames: buildFallbackCallNames(["click"]) },
      fill: { callNames: buildFallbackCallNames(["fill", "fill_form"]) },
      typeText: { callNames: buildFallbackCallNames(["type_text"]) },
      pressKey: { callNames: buildFallbackCallNames(["press_key"]) },
      waitFor: { callNames: buildFallbackCallNames(["wait_for"]) },
      listPages: { callNames: buildFallbackCallNames(["list_pages"]) },
      selectPage: { callNames: buildFallbackCallNames(["select_page"]) },
      evaluate: { callNames: buildFallbackCallNames(["evaluate_script", "evaluate"]) },
      screenshot: { callNames: buildFallbackCallNames(["take_screenshot", "screenshot"]) },
      allChromeTools: [],
    };

    const required = [];
    if (!caps.navigate) required.push("navigate_page|navigate");
    if (!caps.snapshot) required.push("take_snapshot");
    if (!caps.click) required.push("click");
    if (!caps.waitFor) required.push("wait_for");
    if (!caps.fill && !caps.typeText) required.push("fill|type_text");

    if (required.length) {
      return { success: false, message: `Missing required Chrome capabilities: ${required.join(", ")}`, capabilities: caps };
    }

    capabilities = caps;
    return {
      success: true,
      message: fromDiscovery
        ? `Chrome MCP ready (${chromeTools.length} tools)`
        : `Chrome MCP ready (tools.list fallback mode; startup error: ${listErr})`,
      capabilities: caps,
    };
  }

  async function navigate(url) {
    if (!capabilities || !capabilities.navigate) return { success: false, message: "navigate capability unavailable" };
    return callWithVariants(capabilities.navigate, [{ url }, { href: url }]);
  }

  async function snapshot() {
    if (!capabilities || !capabilities.snapshot) return { success: false, message: "snapshot capability unavailable" };
    const result = await callWithVariants(capabilities.snapshot, [{}]);
    if (result.success) {
      // Try to extract structured tree from the response
      const tree = extractSnapshotTree(result.data);
      if (tree) {
        lastSnapshotTree = tree;
        const fullText = buildFullText(tree);
        lastFullSnapshot = fullText;
        result.summary = fullText;
      } else {
        lastSnapshotTree = null;
        lastFullSnapshot = result.summary || "";
      }
    }
    return result;
  }

  async function click(uid) {
    if (!capabilities || !capabilities.click) return { success: false, message: "click capability unavailable" };
    return callWithVariants(capabilities.click, [
      { uid, includeSnapshot: true },
      { elementId: uid, includeSnapshot: true },
      { nodeId: uid, includeSnapshot: true },
    ]);
  }

  async function fill(uid, text) {
    if (!capabilities) return { success: false, message: "capabilities unavailable" };
    if (capabilities.fill) {
      return callWithVariants(capabilities.fill, [
        { uid, value: text, includeSnapshot: true },
        { uid, text, includeSnapshot: true },
        { elementId: uid, value: text, includeSnapshot: true },
      ]);
    }
    if (capabilities.typeText) {
      const clicked = await click(uid);
      if (!clicked.success) return clicked;
      return typeText(text);
    }
    return { success: false, message: "fill/type capability unavailable" };
  }

  async function typeText(text) {
    if (!capabilities || !capabilities.typeText) return { success: false, message: "type_text capability unavailable" };
    return callWithVariants(capabilities.typeText, [
      { text, includeSnapshot: true },
      { value: text, includeSnapshot: true },
    ]);
  }

  async function pressKey(key) {
    if (!capabilities || !capabilities.pressKey) return { success: false, message: "press_key capability unavailable" };
    return callWithVariants(capabilities.pressKey, [
      { key, includeSnapshot: true },
      { keyCode: key, includeSnapshot: true },
    ]);
  }

  async function waitForText(text, timeoutMs) {
    if (!capabilities || !capabilities.waitFor) return { success: false, message: "wait_for capability unavailable" };
    const timeout = Number(timeoutMs);
    const timeoutSec = Number.isFinite(timeout) ? Math.max(1, Math.round(timeout / 1000)) : 5;
    return callWithVariants(capabilities.waitFor, [
      { text, timeout: timeoutSec },
      { text, timeoutMs: Number.isFinite(timeout) ? timeout : 5000 },
      { query: text, timeout: timeoutSec },
    ]);
  }

  async function listPages() {
    if (!capabilities || !capabilities.listPages) return { success: false, message: "list_pages capability unavailable" };
    return callWithVariants(capabilities.listPages, [{}]);
  }

  async function selectPage(index) {
    if (!capabilities || !capabilities.selectPage) return { success: false, message: "select_page capability unavailable" };
    return callWithVariants(capabilities.selectPage, [{ index }, { pageIdx: index }, { pageIndex: index }]);
  }

  async function evaluate(script) {
    if (!capabilities || !capabilities.evaluate) return { success: false, message: "evaluate capability unavailable" };
    return callWithVariants(capabilities.evaluate, [{ function: script }, { script }, { expression: script }]);
  }

  async function screenshot(path) {
    if (!capabilities || !capabilities.screenshot) return { success: false, message: "screenshot capability unavailable" };
    if (path) return callWithVariants(capabilities.screenshot, [{ path }, { filename: path }, {}]);
    return callWithVariants(capabilities.screenshot, [{}]);
  }

  async function detectBlockingOverlay() {
    if (!capabilities || !capabilities.evaluate) {
      return { success: false, message: "evaluate capability unavailable for overlay detection" };
    }

    const res = await evaluate(OVERLAY_DETECTION_SCRIPT);
    if (!res.success) return res;

    const structured = extractAnyObject(res.data);
    if (!structured || typeof structured.blocked !== "boolean") {
      return {
        success: false,
        message: "Could not parse overlay detection result",
        data: res.data,
        summary: res.summary,
      };
    }

    return {
      success: true,
      message: structured.blocked
        ? `Blocking overlay detected (candidates: ${structured.candidateCount || 0})`
        : "No blocking overlay detected",
      data: structured,
      summary: JSON.stringify(structured, null, 2),
    };
  }

  async function dismissBlockingOverlay() {
    if (!capabilities || !capabilities.evaluate) {
      return { success: false, message: "evaluate capability unavailable for overlay dismissal" };
    }

    const res = await evaluate(OVERLAY_DISMISS_SCRIPT);
    if (!res.success) return res;

    const parsed = extractAnyObject(res.data) || {};
    const after = await detectBlockingOverlay();
    const blockedAfter = after.success && after.data ? !!after.data.blocked : null;

    return {
      success: true,
      message: `Dismiss overlay attempted (clicked=${parsed.clickedClose || 0}, hidden=${parsed.hidden || 0}, blockedAfter=${blockedAfter})`,
      data: {
        dismiss: parsed,
        after: after.success ? after.data : null,
      },
      summary: JSON.stringify({
        dismiss: parsed,
        after: after.success ? after.data : null,
      }, null, 2),
    };
  }

  async function extractPhoneNumbers() {
    if (!capabilities || !capabilities.evaluate) {
      return { success: false, message: "evaluate capability unavailable for phone extraction" };
    }

    const res = await evaluate(PHONE_EXTRACTION_SCRIPT);
    if (!res.success) return res;

    const parsed = extractAnyObject(res.data) || {};
    let numbers = [];
    if (Array.isArray(parsed.numbers)) {
      numbers = parsed.numbers
        .map((x) => {
          if (!x) return "";
          if (typeof x === "string") return x;
          if (typeof x === "object" && typeof x.phone === "string") return x.phone;
          return "";
        })
        .filter((x) => x);
    }

    return {
      success: true,
      message: numbers.length
        ? `Found ${numbers.length} phone number candidate(s)`
        : "No phone number candidates found",
      data: {
        count: Array.isArray(parsed.numbers) ? parsed.numbers.length : 0,
        numbers: parsed.numbers || [],
      },
      summary: JSON.stringify({
        count: Array.isArray(parsed.numbers) ? parsed.numbers.length : 0,
        numbers: parsed.numbers || [],
      }, null, 2),
    };
  }

  async function scroll(direction) {
    if (!capabilities || !capabilities.evaluate) {
      return { success: false, message: "evaluate capability unavailable for scroll" };
    }
    const dir = String(direction || "down").toLowerCase();
    let script;
    if (dir === "up")          script = "() => { window.scrollBy(0, -800); }";
    else if (dir === "top")    script = "() => { window.scrollTo(0, 0); }";
    else if (dir === "bottom") script = "() => { window.scrollTo(0, document.body.scrollHeight); }";
    else                       script = "() => { window.scrollBy(0, 800); }";

    const res = await evaluate(script);
    if (!res.success) return res;
    return snapshot();
  }

  async function openFreshTab() {
    if (!capabilities) return { success: false, message: "capabilities not initialized" };
    // Open a fresh tab to avoid touching user's existing tabs
    const newPageCap = capabilities.newPage;
    if (newPageCap) {
      try {
        await callWithVariants(newPageCap, [{ url: "about:blank" }]);
      } catch (_e) {}
    } else if (capabilities.navigate) {
      // Fallback: just navigate to about:blank
      await navigate("about:blank");
    }
    // Bring the newest tab to front
    if (capabilities.listPages && capabilities.selectPage) {
      const pages = await listPages();
      if (pages.success && pages.summary) {
        const lines = String(pages.summary).split("\n");
        let lastId = null;
        for (const line of lines) {
          const m = line.match(/^\s*(\d+):/);
          if (m) lastId = parseInt(m[1], 10);
        }
        if (lastId != null) {
          await callWithVariants(capabilities.selectPage, [
            { pageId: lastId, bringToFront: true },
            { index: lastId, bringToFront: true },
          ]);
        }
      }
    }
    return { success: true, message: "Opened fresh working tab" };
  }

  // Regex-based fallback when no structured tree is available
  function buildPageSummaryFromText(snapshotText) {
    const lines = String(snapshotText || "").split("\n");
    const out = [];
    let totalElements = 0;
    let interactiveCount = 0;

    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed) continue;
      if (/\b(heading |RootWebArea |banner|navigation|main |contentinfo|Iframe )/.test(trimmed)) {
        out.push(line);
        totalElements++;
      } else if (/\b(link |button |searchbox |textbox |input |combobox |checkbox |radio )/.test(trimmed)) {
        out.push(line);
        interactiveCount++;
        totalElements++;
      }
    }

    const header = `PAGE_SUMMARY (${totalElements} elements, ${interactiveCount} interactive, ${lines.length} total lines):`;
    return header + "\n" + out.join("\n");
  }

  function filterSnapshot(pattern) {
    if (!lastFullSnapshot) return { success: false, message: "No snapshot available. Use snapshot first." };
    const lines = lastFullSnapshot.split("\n");
    const re = new RegExp(pattern, "i");
    const matched = [];
    for (let i = 0; i < lines.length; i++) {
      if (re.test(lines[i])) {
        // Include 1 line before for parent context
        if (i > 0 && matched[matched.length - 1] !== lines[i - 1]) {
          matched.push(lines[i - 1]);
        }
        matched.push(lines[i]);
        // Include 1 line after for child context
        if (i + 1 < lines.length) {
          matched.push(lines[i + 1]);
          i++; // skip the next line since we already added it
        }
      }
    }
    if (!matched.length) {
      return {
        success: true,
        message: `No lines matched pattern "${pattern}"`,
        summary: `No matches for "${pattern}" in ${lines.length} lines.`,
      };
    }
    return {
      success: true,
      message: `Found ${matched.length} lines matching "${pattern}"`,
      summary: matched.join("\n"),
    };
  }

  function getFullSnapshot() {
    if (!lastFullSnapshot) return { success: false, message: "No snapshot available." };
    return { success: true, message: "Full snapshot", summary: lastFullSnapshot };
  }

  async function captureState() {
    const snap = await snapshot();
    if (!snap.success) {
      return {
        success: false,
        hash: "",
        summary: `Snapshot failed: ${snap.message}`,
      };
    }

    const lines = [];

    // Send compact summary (interactive + content + landmarks only) to reduce token usage
    const compactView = lastSnapshotTree ? buildCompactSummary(lastSnapshotTree) : snap.summary;
    lines.push(`BROWSER_STATE:\n${compactView}`);

    if (capabilities.listPages) {
      const pages = await listPages();
      if (pages.success) {
        lines.push(`PAGES:\n${pages.summary}`);
      }
    }

    if (capabilities.evaluate) {
      const docState = await evaluate("() => ({title: document.title, url: location.href})");
      if (docState.success) {
        lines.push(`DOCUMENT:\n${docState.summary}`);
      }
    }

    const summary = lines.join("\n\n");
    return {
      success: true,
      hash: hashText(summary),
      summary,
    };
  }

  function capabilitySummary() {
    if (!capabilities) return "Capabilities not initialized";
    const lines = [];
    for (const [k, v] of Object.entries(capabilities)) {
      if (k === "allChromeTools") continue;
      if (!v) {
        lines.push(`${k}: unavailable`);
        continue;
      }
      const names = Array.isArray(v.callNames) ? v.callNames : (v.callName ? [v.callName] : []);
      lines.push(`${k}: ${names.join(" | ")}`);
    }
    return lines.join("\n");
  }

  return {
    discoverCapabilities,
    capabilitySummary,
    navigate,
    snapshot,
    click,
    fill,
    typeText,
    pressKey,
    waitForText,
    listPages,
    selectPage,
    evaluate,
    screenshot,
    scroll,
    detectBlockingOverlay,
    dismissBlockingOverlay,
    extractPhoneNumbers,
    captureState,
    openFreshTab,
    filterSnapshot,
    getFullSnapshot,
  };
}
