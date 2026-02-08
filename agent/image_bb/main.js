// ./target/release/hugind agent run agent/image_bb --image /Users/adel/Downloads/pc.jpg --labels "person,car"

function parseArgs(input) {
  const args = (input && Array.isArray(input.args)) ? input.args.slice() : [];
  const out = {
    image_url: "",
    prompt: "",
    labels: [],
    return_image_size: true
  };

  let stopFlags = false;

  for (let i = 0; i < args.length; i += 1) {
    const key = args[i];

    if (!stopFlags && key === "--") {
      stopFlags = true;
      continue;
    }

    if (!stopFlags && (key === "--image" || key === "--image-url")) {
      out.image_url = args[i + 1] || "";
      i += 1;
      continue;
    }

    if (!stopFlags && key === "--prompt") {
      out.prompt = args[i + 1] || "";
      i += 1;
      continue;
    }

    if (!stopFlags && key === "--labels") {
      const raw = args[i + 1] || "";
      const parts = raw.split(",").map(s => s.trim()).filter(Boolean);
      if (parts.length) out.labels.push(...parts);
      i += 1;
      continue;
    }

    if (!stopFlags && key === "--no-image-size") {
      out.return_image_size = false;
      continue;
    }

    // Positional image (first non-flag token)
    if (!out.image_url && key && (stopFlags || !String(key).startsWith("--"))) {
      out.image_url = String(key);
      continue;
    }
  }

  // De-dupe labels, preserve order
  if (out.labels.length > 1) {
    const seen = Object.create(null);
    const uniq = [];
    for (const l of out.labels) {
      const k = String(l);
      if (!seen[k]) {
        seen[k] = true;
        uniq.push(k);
      }
    }
    out.labels = uniq;
  }

  return out;
}

function buildPrompt(opts) {
  const labels = (opts.labels && opts.labels.length)
    ? opts.labels.map(s => String(s).trim()).filter(Boolean)
    : [];

  // Qwen3-VL responds best when you tell it to "locate" or "detect" 
  // and specify the exact JSON key "bbox_2d".
  const labelRule = labels.length
    ? `Target objects: ${labels.join(", ")}.`
    : "Detect the main objects in the image.";

  const taskHint = (opts.prompt && String(opts.prompt).trim())
    ? `Task: ${String(opts.prompt).trim()}`
    : "Task: Return bounding boxes for all requested objects.";

  return [
    "You are an expert visual grounding model.",
    taskHint,
    "Output MUST be a JSON object. Do not include markdown code blocks or text outside the JSON.",
    "JSON Schema:",
    "{",
    "  \"boxes\": [",
    "    {",
    "      \"label\": \"string\",",
    "      \"bbox_2d\": [x1, y1, x2, y2]",
    "    }",
    "  ]",
    "}",
    "",
    "Coordinate Rules:",
    "- Use a 1000x1000 relative coordinate system.",
    "- [0, 0] is top-left, [1000, 1000] is bottom-right.",
    "- Coordinates [x1, y1, x2, y2] MUST be integers.",
    "- Order: x_min, y_min, x_max, y_max.",
    labelRule
  ].join("\n");
}


function inferMimeType(path) {
  const s = String(path || "");
  // Strip query/hash just in case you pass paths like "file.png?x=1"
  const clean = s.split("#")[0].split("?")[0];
  const lower = clean.toLowerCase();

  if (lower.endsWith(".png")) return "image/png";
  if (lower.endsWith(".jpg") || lower.endsWith(".jpeg")) return "image/jpeg";
  if (lower.endsWith(".webp")) return "image/webp";
  if (lower.endsWith(".gif")) return "image/gif";
  if (lower.endsWith(".bmp")) return "image/bmp";
  return "application/octet-stream";
}

// Fast-ish base64 in pure JS: push chars into an array and join once.
function base64Encode(bytes) {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const out = [];
  let i = 0;

  for (; i + 2 < bytes.length; i += 3) {
    const n = (bytes[i] << 16) | (bytes[i + 1] << 8) | bytes[i + 2];
    out.push(
      alphabet[(n >> 18) & 63],
      alphabet[(n >> 12) & 63],
      alphabet[(n >> 6) & 63],
      alphabet[n & 63]
    );
  }

  if (i < bytes.length) {
    const b1 = bytes[i];
    const b2 = i + 1 < bytes.length ? bytes[i + 1] : 0;
    const n = (b1 << 16) | (b2 << 8);

    out.push(alphabet[(n >> 18) & 63], alphabet[(n >> 12) & 63]);

    if (i + 1 < bytes.length) {
      out.push(alphabet[(n >> 6) & 63], "=");
    } else {
      out.push("=", "=");
    }
  }

  return out.join("");
}

// If your embedder provides btoa (not guaranteed in QuickJS), use it.
function bytesToBinaryString(bytes) {
  // Chunk to avoid stack overflow with apply() on big arrays
  const CHUNK = 0x8000;
  let s = "";
  for (let i = 0; i < bytes.length; i += CHUNK) {
    const sub = bytes.subarray ? bytes.subarray(i, i + CHUNK) : bytes.slice(i, i + CHUNK);
    s += String.fromCharCode.apply(null, sub);
  }
  return s;
}

function base64EncodeMaybeNative(bytes) {
  if (typeof btoa === "function") {
    return btoa(bytesToBinaryString(bytes));
  }
  return base64Encode(bytes);
}

function normalizeImageUrl(imageUrl) {
  if (!imageUrl) return "";
  const s = String(imageUrl);

  if (s.startsWith("http://") || s.startsWith("https://")) return s;
  if (s.startsWith("data:")) return s;

  const bytes = fs.read_bytes(s);
  const mime = inferMimeType(s);
  const b64 = base64EncodeMaybeNative(bytes);
  return `data:${mime};base64,${b64}`;
}

function safeJsonParse(raw) {
  // raw might already be an object depending on host; handle both.
  if (raw && typeof raw === "object") return raw;
  return JSON.parse(String(raw));
}

export default async function main(input) {
  const opts = parseArgs(input);

  if (!opts.image_url) {
    set_result({ error: "Missing image. Use --image <path>." });
    return;
  }

  try {
    opts.image_url = normalizeImageUrl(opts.image_url);
  } catch (e) {
    set_result({ error: "Failed to load image.", details: String(e) });
    return;
  }

  const prompt = buildPrompt(opts);

  const request = {
    messages: [
      {
        role: "user",
        content: [
          { type: "text", text: prompt },
          { type: "image_url", image_url: { url: opts.image_url } }
        ]
      }
    ],
    // Qwen3-VL is optimized for structured JSON outputs
    response_format: { type: "json_object" },
    temperature: 0.1, // Lower temperature is better for precise coordinate tasks
    max_tokens: 2048,
    on_token: (delta) => {
      print_raw(delta);
    }
  };

  try {
    const raw = await llm.chat_stream(request);
    const parsed = safeJsonParse(raw);
    
    // Optional: Return image size if needed for scaling back to original pixels
    if (opts.return_image_size) {
      // In many environments, you'd calculate this from the buffer/file
      // parsed.meta = { coordinate_system: "1000x1000" };
    }
    
    set_result(parsed);
  } catch (e) {
    set_result({ error: "Inference or parsing failed.", details: String(e) });
  }
}