// ./target/release/hugind agent run agent/ocr --image /Users/adel/Downloads/18zjgwovgbhg1.jpeg --prompt "only read the title"

function parseArgs(input) {
    const args = (input && Array.isArray(input.args)) ? input.args.slice() : [];
    const out = {
        image_url: "",
        prompt: "",
        include_lines: false
    };

    for (let i = 0; i < args.length; i += 1) {
        const key = args[i];
        if (key === "--image" || key === "--image-url") {
            out.image_url = args[i + 1] || "";
            i += 1;
        } else if (key === "--prompt") {
            out.prompt = args[i + 1] || "";
            i += 1;
        } else if (key === "--no-lines") {
            out.include_lines = false;
        } else if (!out.image_url && key && !key.startsWith("--")) {
            out.image_url = key;
        }
    }

    return out;
}

function buildPrompt(opts) {
    const taskHint = opts.prompt && opts.prompt.trim()
        ? `User request: ${opts.prompt.trim()}`
        : "User request: Extract all readable text with structure.";

    return [
        "You are an OCR engine that returns structured JSON.",
        taskHint,
        "Output MUST be a JSON object. Do not include markdown code blocks or text outside the JSON.",
        "JSON Schema:",
        "{",
        "  \"blocks\": [",
        "    {",
        "      \"block_type\": \"text\" | \"table\" | \"form\" | \"other\",",
        "      \"text\": string,",
        "      \"bbox_2d\": [x1, y1, x2, y2],",
        "    }",
        "  ]",
        "}",
        "Coordinate Rules:",
        "- Use a 1000x1000 relative coordinate system.",
        "- [0, 0] is top-left, [1000, 1000] is bottom-right.",
        "- Coordinates [x1, y1, x2, y2] MUST be integers.",
        "- Order: x_min, y_min, x_max, y_max.",
        "Confidence is 0-1."
    ].join("\n");
}

function inferMimeType(path) {
    const lower = (path || "").toLowerCase();
    if (lower.endsWith(".png")) return "image/png";
    if (lower.endsWith(".jpg") || lower.endsWith(".jpeg")) return "image/jpeg";
    if (lower.endsWith(".webp")) return "image/webp";
    if (lower.endsWith(".gif")) return "image/gif";
    if (lower.endsWith(".bmp")) return "image/bmp";
    return "application/octet-stream";
}

function base64Encode(bytes) {
    const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let out = "";
    let i = 0;
    for (; i + 2 < bytes.length; i += 3) {
        const n = (bytes[i] << 16) | (bytes[i + 1] << 8) | bytes[i + 2];
        out += alphabet[(n >> 18) & 63];
        out += alphabet[(n >> 12) & 63];
        out += alphabet[(n >> 6) & 63];
        out += alphabet[n & 63];
    }
    if (i < bytes.length) {
        const b1 = bytes[i];
        const b2 = i + 1 < bytes.length ? bytes[i + 1] : 0;
        const n = (b1 << 16) | (b2 << 8);
        out += alphabet[(n >> 18) & 63];
        out += alphabet[(n >> 12) & 63];
        if (i + 1 < bytes.length) {
            out += alphabet[(n >> 6) & 63];
            out += "=";
        } else {
            out += "==";
        }
    }
    return out;
}

function normalizeImageUrl(imageUrl) {
    if (!imageUrl) return "";
    if (imageUrl.startsWith("http://") || imageUrl.startsWith("https://")) {
        return imageUrl;
    }
    if (imageUrl.startsWith("data:")) {
        return imageUrl;
    }
    const bytes = fs.read_bytes(imageUrl);
    const mime = inferMimeType(imageUrl);
    const b64 = base64Encode(bytes);
    return `data:${mime};base64,${b64}`;
}

export default async function main(input) {
    const opts = parseArgs(input);
    if (!opts.image_url) {
        const msg = "Missing image. Provide --image <path|data-url|http-url>.";
        set_result({ error: msg });
        print(JSON.stringify({ error: msg }, null, 2));
        return;
    }

    try {
        opts.image_url = normalizeImageUrl(opts.image_url);
    } catch (e) {
        const err = { error: "Failed to load image file.", details: String(e) };
        set_result(err);
        print(JSON.stringify(err, null, 2));
        return;
    }

    const prompt = buildPrompt(opts);
    let stream_preview = "";
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
        response_format: { type: "json_object" },
        temperature: 0.2,
        max_tokens: 1400,
        on_token: (delta) => {
            stream_preview += delta;
            print_raw(delta);
        }
    };

    let raw;
    try {
        raw = await llm.chat_stream(request);
    } catch (e) {
        const err = { error: "llm.chat_stream failed.", details: String(e) };
        set_result(err);
        print(JSON.stringify(err, null, 2));
        return;
    }

    try {
        const parsed = JSON.parse(raw);
        set_result(parsed);
    } catch (e) {
        const err = { error: "Failed to parse JSON response.", raw: String(raw), details: String(e) };
        set_result(err);
        print(JSON.stringify(err, null, 2));
    }
}
