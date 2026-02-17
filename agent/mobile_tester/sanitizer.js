function decodeXml(s) {
  return s
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&");
}

function parseAttrs(tag) {
  const out = {};
  const re = /(\S+)="([\s\S]*?)"/g;
  let m;
  while ((m = re.exec(tag)) !== null) {
    out[m[1]] = decodeXml(m[2]);
  }
  return out;
}

function parseBounds(bounds) {
  const m = bounds && bounds.match(/\[(\d+),(\d+)\]\[(\d+),(\d+)\]/);
  if (!m) return null;
  const x1 = Number(m[1]);
  const y1 = Number(m[2]);
  const x2 = Number(m[3]);
  const y2 = Number(m[4]);
  const width = x2 - x1;
  const height = y2 - y1;
  if (width <= 0 || height <= 0) return null;
  return {
    center: [Math.floor((x1 + x2) / 2), Math.floor((y1 + y2) / 2)],
    size: [width, height],
  };
}

export function computeScreenHash(elements) {
  return elements
    .map((e) => `${e.id}|${e.text}|${e.center[0]},${e.center[1]}|${e.enabled}|${e.checked}`)
    .join(";");
}

export function getInteractiveElements(xmlContent) {
  const elements = [];
  const stack = ["root"];
  const tokenRe = /<\/?node\b[^>]*\/?>/g;
  let m;

  while ((m = tokenRe.exec(xmlContent)) !== null) {
    const token = m[0];
    const close = token.startsWith("</");
    const selfClosing = /\/\s*>$/.test(token);

    if (close) {
      if (stack.length > 1) stack.pop();
      continue;
    }

    const attrs = parseAttrs(token);
    const elementClass = attrs["class"] || "";
    const isClickable = attrs["clickable"] === "true";
    const isLongClickable = attrs["long-clickable"] === "true";
    const isScrollable = attrs["scrollable"] === "true";
    const isEnabled = attrs["enabled"] !== "false";
    const isChecked = attrs["checked"] === "true";
    const isFocused = attrs["focused"] === "true";
    const isSelected = attrs["selected"] === "true";
    const isPassword = attrs["password"] === "true";

    const isEditable =
      elementClass.indexOf("EditText") !== -1 ||
      elementClass.indexOf("AutoCompleteTextView") !== -1 ||
      attrs["editable"] === "true";

    const text = attrs["text"] || "";
    const desc = attrs["content-desc"] || "";
    const resourceId = attrs["resource-id"] || "";
    const hint = attrs["hint"] || "";
    const nodeLabel =
      text ||
      desc ||
      (resourceId.indexOf("/") !== -1 ? resourceId.split("/").pop() : resourceId) ||
      (elementClass.indexOf(".") !== -1 ? elementClass.split(".").pop() : elementClass) ||
      "node";

    const bounds = attrs["bounds"] || "";
    const isInteractive = isClickable || isEditable || isLongClickable || isScrollable;
    const hasContent = !!(text || desc);

    if (bounds && (isInteractive || hasContent)) {
      const parsed = parseBounds(bounds);
      if (parsed) {
        let action = "read";
        if (isEditable) action = "type";
        else if (isLongClickable && !isClickable) action = "longpress";
        else if (isScrollable && !isClickable) action = "scroll";
        else if (isClickable) action = "tap";

        elements.push({
          id: resourceId,
          text: text || desc,
          type: elementClass.indexOf(".") !== -1 ? elementClass.split(".").pop() : elementClass,
          bounds,
          center: parsed.center,
          size: parsed.size,
          clickable: isClickable,
          editable: isEditable,
          enabled: isEnabled,
          checked: isChecked,
          focused: isFocused,
          selected: isSelected,
          scrollable: isScrollable,
          longClickable: isLongClickable,
          password: isPassword,
          hint,
          action,
          parent: stack[stack.length - 1],
          depth: stack.length - 1,
        });
      }
    }

    if (!selfClosing) {
      stack.push(nodeLabel);
    }
  }

  return elements;
}

function scoreElement(el) {
  let score = 0;
  if (el.enabled) score += 10;
  if (el.editable) score += 8;
  if (el.focused) score += 6;
  if (el.clickable || el.longClickable) score += 5;
  if (el.text) score += 3;
  return score;
}

function compactElement(el) {
  const compact = { text: el.text, center: el.center, action: el.action };
  if (!el.enabled) compact.enabled = false;
  if (el.checked) compact.checked = true;
  if (el.focused) compact.focused = true;
  if (el.hint) compact.hint = el.hint;
  if (el.editable) compact.editable = true;
  if (el.scrollable) compact.scrollable = true;
  return compact;
}

export function filterElements(elements, limit) {
  const seen = {};
  for (const el of elements) {
    const bucketX = Math.round(el.center[0] / 5) * 5;
    const bucketY = Math.round(el.center[1] / 5) * 5;
    const key = `${bucketX},${bucketY}`;
    const existing = seen[key];
    if (!existing || scoreElement(el) > scoreElement(existing)) {
      seen[key] = el;
    }
  }
  const deduped = Object.values(seen);
  deduped.sort((a, b) => scoreElement(b) - scoreElement(a));
  return deduped.slice(0, limit).map(compactElement);
}
