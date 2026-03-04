/*
dataset_labeler

Runtime contract:
- Dataset root is fs.cwd() (run with --cwd /path/to/dataset)
- Required:
  - images/train
  - meta/classes.txt
- Optional:
  - images/val
  - images/test
- Outputs:
  - labels/<split>/*.txt (YOLO normalized labels)
  - meta/raw/<split>/*.json (raw model output + debug context)
  - meta/dataset_labeler.log (run logs)
*/

const AGENT_NAME = "dataset_labeler";
const LOG_FILENAME = "dataset_labeler.log";

const runtime = {
  logPath: "",
  logWriteFailed: false
};

function parseRunOptions(input) {
  const args = input && Array.isArray(input.args) ? input.args : [];
  const opts = {
    resume: false,
    retryFailedOnly: false,
    listFailedKind: "",
    unknownFlags: []
  };

  for (let i = 0; i < args.length; i += 1) {
    const a = String(args[i] || "").trim();
    if (!a.startsWith("--")) continue;

    if (a === "--resume") {
      opts.resume = true;
      continue;
    }
    if (a === "--retry-failed-only" || a === "--retry-failed") {
      opts.retryFailedOnly = true;
      opts.resume = true;
      continue;
    }
    if (a === "--list-failed") {
      opts.listFailedKind = "image";
      continue;
    }
    if (a.startsWith("--list-failed=")) {
      const mode = a.slice("--list-failed=".length).trim().toLowerCase();
      if (mode === "image" || mode === "raw" || mode === "label" || mode === "all") {
        opts.listFailedKind = mode;
      } else {
        opts.unknownFlags.push(a);
      }
      continue;
    }
    opts.unknownFlags.push(a);
  }

  return opts;
}

function joinPath(a, b) {
  const left = String(a || "");
  const right = String(b || "");
  if (!left) return right;
  if (!right) return left;
  return left.endsWith("/") ? left + right : left + "/" + right;
}

function dirname(path) {
  const s = String(path || "");
  const i = s.lastIndexOf("/");
  if (i <= 0) return ".";
  return s.slice(0, i);
}

function basename(path) {
  const s = String(path || "");
  const i = s.lastIndexOf("/");
  return i < 0 ? s : s.slice(i + 1);
}

function withoutExt(path) {
  const base = basename(path);
  const idx = base.lastIndexOf(".");
  if (idx <= 0) return base;
  return base.slice(0, idx);
}

function replaceExtension(path, extWithDot) {
  const dir = dirname(path);
  const stem = withoutExt(path);
  return joinPath(dir, `${stem}${extWithDot}`);
}

function toRelativePath(fullPath, prefixDir) {
  const full = String(fullPath || "");
  const pref = String(prefixDir || "");
  if (full === pref) return "";
  const p = pref.endsWith("/") ? pref : `${pref}/`;
  if (full.startsWith(p)) return full.slice(p.length);
  return basename(full);
}

function nowIso() {
  try {
    return new Date().toISOString();
  } catch (_err) {
    return "unknown-time";
  }
}

function writeLogLine(level, message) {
  const msg = String(message || "");
  const line = `${nowIso()} [${level}] ${msg}`;
  print(`[${AGENT_NAME}][${level}] ${msg}`);

  if (!runtime.logPath) return;
  try {
    fs.append_text(runtime.logPath, `${line}\n`);
  } catch (e) {
    if (!runtime.logWriteFailed) {
      runtime.logWriteFailed = true;
      print(`[${AGENT_NAME}][WARN] Failed to write log file '${runtime.logPath}': ${String(e)}`);
    }
  }
}

function logInfo(msg) {
  writeLogLine("INFO", msg);
}

function logWarn(msg) {
  writeLogLine("WARN", msg);
}

function logError(msg) {
  writeLogLine("ERROR", msg);
}

function safeJsonParse(raw) {
  if (raw && typeof raw === "object") return raw;
  return JSON.parse(String(raw));
}

function assertExistsDir(path, label) {
  logInfo(`Checking required directory: ${label} -> ${path}`);
  if (!fs.exists(path)) {
    throw new Error(`${label} is missing: ${path}`);
  }
  if (!fs.is_dir(path)) {
    throw new Error(`${label} is not a directory: ${path}`);
  }
}

function assertExistsFile(path, label) {
  logInfo(`Checking required file: ${label} -> ${path}`);
  if (!fs.exists(path)) {
    throw new Error(`${label} is missing: ${path}`);
  }
  if (fs.is_dir(path)) {
    throw new Error(`${label} must be a file, got directory: ${path}`);
  }
}

function ensureDir(path) {
  if (!fs.exists(path)) {
    fs.mkdir(path, true);
    logInfo(`Created directory: ${path}`);
  } else if (!fs.is_dir(path)) {
    throw new Error(`Path exists but is not a directory: ${path}`);
  }
}

function ensureParentDir(filePath) {
  const dir = dirname(filePath);
  ensureDir(dir);
}

function isImageFile(path) {
  const s = String(path || "").toLowerCase();
  return (
    s.endsWith(".png") ||
    s.endsWith(".jpg") ||
    s.endsWith(".jpeg") ||
    s.endsWith(".webp") ||
    s.endsWith(".bmp") ||
    s.endsWith(".gif")
  );
}

function listSplitImages(splitDir) {
  const images = [];
  const stack = [splitDir];
  let dirsVisited = 0;

  while (stack.length > 0) {
    const dir = stack.pop();
    dirsVisited += 1;
    const names = safeJsonParse(fs.list_dir(dir));
    for (let i = 0; i < names.length; i += 1) {
      const fullPath = joinPath(dir, String(names[i]));
      if (fs.is_dir(fullPath)) {
        stack.push(fullPath);
      } else if (isImageFile(fullPath)) {
        images.push(fullPath);
      }
    }
  }

  images.sort();
  logInfo(`Scanned ${splitDir}: dirs=${dirsVisited}, images=${images.length}`);
  if (!images.length) {
    logWarn(`No images found under ${splitDir}`);
  }
  return images;
}

function validateClassName(name, lineNo) {
  const trimmed = String(name || "").trim();
  if (!trimmed) {
    throw new Error(`classes.txt line ${lineNo}: empty class name`);
  }
  if (!/^[A-Za-z0-9][A-Za-z0-9 _.-]*$/.test(trimmed)) {
    throw new Error(
      `classes.txt line ${lineNo}: invalid class name '${trimmed}' (allowed: letters, digits, space, _, -, .)`
    );
  }
  return trimmed;
}

function parseClasses(classesPath) {
  logInfo(`Parsing classes: ${classesPath}`);
  const text = fs.read_text(classesPath);
  const lines = String(text || "").split("\n");
  const classes = [];
  const seen = {};

  for (let i = 0; i < lines.length; i += 1) {
    const lineNo = i + 1;
    const raw = String(lines[i] || "").trim();
    if (!raw || raw.startsWith("#")) continue;

    let namePart = raw;
    let descriptionPart = "";
    const pipeSep = raw.indexOf("|");
    if (pipeSep >= 0) {
      namePart = raw.slice(0, pipeSep);
      descriptionPart = raw.slice(pipeSep + 1);
    } else {
      // Also support "class_name - description" for convenience.
      const dashSep = raw.indexOf(" - ");
      if (dashSep >= 0) {
        namePart = raw.slice(0, dashSep);
        descriptionPart = raw.slice(dashSep + 3);
      }
    }

    const name = validateClassName(namePart, lineNo);
    const key = name.toLowerCase();
    if (seen[key]) {
      throw new Error(`classes.txt line ${lineNo}: duplicate class name '${name}'`);
    }
    seen[key] = true;

    const description = String(descriptionPart || "").trim();
    classes.push({
      id: classes.length,
      name,
      description
    });
  }

  if (!classes.length) {
    throw new Error("classes.txt has no valid class definitions");
  }

  for (let i = 0; i < classes.length; i += 1) {
    logInfo(`Class[${classes[i].id}] '${classes[i].name}' desc='${classes[i].description}'`);
  }
  return classes;
}

function inferMimeType(path) {
  const s = String(path || "").toLowerCase();
  if (s.endsWith(".png")) return "image/png";
  if (s.endsWith(".jpg") || s.endsWith(".jpeg")) return "image/jpeg";
  if (s.endsWith(".webp")) return "image/webp";
  if (s.endsWith(".gif")) return "image/gif";
  if (s.endsWith(".bmp")) return "image/bmp";
  return "application/octet-stream";
}

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
    if (i + 1 < bytes.length) out.push(alphabet[(n >> 6) & 63], "=");
    else out.push("=", "=");
  }

  return out.join("");
}

function bytesToBinaryString(bytes) {
  const CHUNK = 0x8000;
  let s = "";
  for (let i = 0; i < bytes.length; i += CHUNK) {
    const sub = bytes.subarray ? bytes.subarray(i, i + CHUNK) : bytes.slice(i, i + CHUNK);
    s += String.fromCharCode.apply(null, sub);
  }
  return s;
}

function base64EncodeMaybeNative(bytes) {
  if (typeof btoa === "function") return btoa(bytesToBinaryString(bytes));
  return base64Encode(bytes);
}

function normalizeImageUrl(imagePath) {
  const s = String(imagePath || "");
  if (!s) return "";
  if (s.startsWith("http://") || s.startsWith("https://") || s.startsWith("data:")) return s;
  const bytes = fs.read_bytes(s);
  const mime = inferMimeType(s);
  const b64 = base64EncodeMaybeNative(bytes);
  return `data:${mime};base64,${b64}`;
}

function buildDetectionPrompt(classes) {
  const classLines = [];
  for (let i = 0; i < classes.length; i += 1) {
    const c = classes[i];
    const desc = c.description ? ` - ${c.description}` : "";
    classLines.push(`- ${c.name}${desc}`);
  }

  return [
    "You are a vision annotation assistant for object detection dataset creation.",
    "Detect only objects that match the allowed classes below.",
    "Allowed classes:",
    classLines.join("\n"),
    "",
    "Output rules:",
    "1) Return ONLY valid JSON.",
    "2) Use this exact schema:",
    "{",
    "  \"detections\": [",
    "    {",
    "      \"class_name\": \"string (must match one allowed class exactly)\",",
    "      \"bbox\": [x1, y1, x2, y2],",
    "      \"confidence\": 0.0",
    "    }",
    "  ]",
    "}",
    "3) bbox coordinates MUST be integers in a 1000x1000 reference space.",
    "4) [0,0] is top-left and [1000,1000] is bottom-right.",
    "5) Use empty detections array when nothing matches.",
    "6) Keep output compact; do not include explanation fields.",
    "7) Do not include any prose or markdown."
  ].join("\n");
}

function parseModelJson(raw) {
  if (raw && typeof raw === "object") return raw;
  const text = String(raw || "").trim();

  if (!text) throw new Error("Model response is empty");

  try {
    return JSON.parse(text);
  } catch (_err) {}

  const fenced = text.match(/```(?:json)?\s*([\s\S]*?)```/i);
  if (fenced && fenced[1]) {
    try {
      return JSON.parse(fenced[1].trim());
    } catch (_err) {}
  }

  const first = text.indexOf("{");
  const last = text.lastIndexOf("}");
  if (first >= 0 && last > first) {
    const slice = text.slice(first, last + 1);
    try {
      return JSON.parse(slice);
    } catch (_err) {}
  }

  const preview = text.length > 800 ? `${text.slice(0, 800)}...` : text;
  throw new Error(`Failed to parse model JSON response. Preview: ${preview}`);
}

function toNumber(v) {
  const n = Number(v);
  return Number.isFinite(n) ? n : NaN;
}

function clamp(v, lo, hi) {
  return Math.max(lo, Math.min(hi, v));
}

function extractBBox(det) {
  if (!det || typeof det !== "object") return null;
  if (Array.isArray(det.bbox)) return det.bbox;
  if (Array.isArray(det.bbox_xyxy)) return det.bbox_xyxy;
  if (Array.isArray(det.bbox_2d)) return det.bbox_2d;
  return null;
}

function bboxToYolo(bbox) {
  if (!Array.isArray(bbox) || bbox.length !== 4) {
    throw new Error(`Invalid bbox: expected [x1,y1,x2,y2], got ${JSON.stringify(bbox)}`);
  }
  let x1 = toNumber(bbox[0]);
  let y1 = toNumber(bbox[1]);
  let x2 = toNumber(bbox[2]);
  let y2 = toNumber(bbox[3]);
  if ([x1, y1, x2, y2].some((n) => Number.isNaN(n))) {
    throw new Error(`Invalid bbox values: ${JSON.stringify(bbox)}`);
  }

  const maxAbs = Math.max(Math.abs(x1), Math.abs(y1), Math.abs(x2), Math.abs(y2));
  if (maxAbs > 1.5) {
    x1 /= 1000.0;
    y1 /= 1000.0;
    x2 /= 1000.0;
    y2 /= 1000.0;
  }

  x1 = clamp(x1, 0, 1);
  y1 = clamp(y1, 0, 1);
  x2 = clamp(x2, 0, 1);
  y2 = clamp(y2, 0, 1);

  if (x2 < x1) {
    const t = x1;
    x1 = x2;
    x2 = t;
  }
  if (y2 < y1) {
    const t = y1;
    y1 = y2;
    y2 = t;
  }

  const w = x2 - x1;
  const h = y2 - y1;
  if (w <= 0 || h <= 0) {
    return null;
  }

  const cx = x1 + w / 2.0;
  const cy = y1 + h / 2.0;
  return [cx, cy, w, h];
}

function fmt6(v) {
  return Number(v).toFixed(6);
}

function classIndexMap(classes) {
  const map = {};
  for (let i = 0; i < classes.length; i += 1) {
    const c = classes[i];
    map[c.name.toLowerCase()] = c.id;
  }
  return map;
}

function normalizeModelDetections(parsed) {
  if (!parsed || typeof parsed !== "object") {
    throw new Error("Model JSON must be an object");
  }
  const list = parsed.detections;
  if (!Array.isArray(list)) {
    throw new Error("Model JSON must contain 'detections' array");
  }
  return list;
}

function errorToObject(err) {
  if (err && typeof err === "object") {
    const message = err.message !== undefined ? String(err.message) : String(err);
    const stack = err.stack ? String(err.stack) : null;
    const rawModelOutput =
      err.raw_model_output !== undefined && err.raw_model_output !== null
        ? String(err.raw_model_output)
        : null;
    return { message, stack, raw_model_output: rawModelOutput };
  }
  return { message: String(err), stack: null, raw_model_output: null };
}

function writeJsonFile(path, value) {
  ensureParentDir(path);
  fs.write_text(path, `${JSON.stringify(value, null, 2)}\n`);
}

function getArtifactPaths(job, labelsRoot, rawRoot) {
  const labelsPath = replaceExtension(
    joinPath(joinPath(labelsRoot, job.split), job.relativePath),
    ".txt"
  );
  const rawPath = replaceExtension(joinPath(joinPath(rawRoot, job.split), job.relativePath), ".json");
  return { labelsPath, rawPath };
}

function inspectJobState(job, labelsRoot, rawRoot) {
  const paths = getArtifactPaths(job, labelsRoot, rawRoot);
  const labelExists = fs.exists(paths.labelsPath) && !fs.is_dir(paths.labelsPath);
  const rawExists = fs.exists(paths.rawPath) && !fs.is_dir(paths.rawPath);

  if (!labelExists && !rawExists) {
    return { status: "new", reason: "no_artifacts", paths };
  }

  if (!rawExists && labelExists) {
    return { status: "failed", reason: "label_without_raw", paths };
  }

  if (rawExists) {
    try {
      const rawObj = safeJsonParse(fs.read_text(paths.rawPath));
      if (rawObj && typeof rawObj === "object" && rawObj.error) {
        return { status: "failed", reason: "raw_has_error", paths };
      }
      if (!labelExists) {
        return { status: "failed", reason: "raw_without_label", paths };
      }
      return { status: "done", reason: "raw_ok_and_label_exists", paths };
    } catch (_err) {
      return { status: "failed", reason: "raw_unparseable", paths };
    }
  }

  return { status: "new", reason: "fallback", paths };
}

async function annotateImage(job, classes, classMap, labelsRoot, rawRoot, promptText) {
  const imageDataUrl = normalizeImageUrl(job.imagePath);
  let raw = "";
  let parsed = null;
  let detections = null;
  let parseErr = null;
  let bestPartialOutput = "";

  for (let attempt = 1; attempt <= 2; attempt += 1) {
    let partial = "";
    const attemptPrompt =
      attempt === 1
        ? promptText
        : [
            promptText,
            "",
            "IMPORTANT RETRY: your previous output was invalid/truncated.",
            "Return complete valid JSON only with properly closed braces."
          ].join("\n");

    const request = {
      messages: [
        {
          role: "user",
          content: [
            { type: "text", text: attemptPrompt },
            { type: "image_url", image_url: { url: imageDataUrl } }
          ]
        }
      ],
      response_format: { type: "json_object" },
      temperature: 0.0,
      max_tokens: 4096,
      on_token: (delta) => {
        const d = String(delta || "");
        partial += d;
      }
    };

    try {
      raw = await llm.chat_stream(request);
      parsed = parseModelJson(raw);
      detections = normalizeModelDetections(parsed);
      parseErr = null;
      if (attempt > 1) {
        logWarn(`Recovered parse with retry for ${job.relativePath}`);
      }
      break;
    } catch (e) {
      parseErr = errorToObject(e);
      if (!parseErr.raw_model_output && partial) {
        parseErr.raw_model_output = partial;
      }
      if (partial && partial.length > bestPartialOutput.length) {
        bestPartialOutput = partial;
      }
      const partialLen = parseErr.raw_model_output ? parseErr.raw_model_output.length : 0;
      logWarn(
        `Attempt ${attempt}/2 failed for ${job.relativePath}: ${parseErr.message} (partial_len=${partialLen})`
      );
    }
  }

  if (parseErr) {
    if (!parseErr.raw_model_output && bestPartialOutput) {
      parseErr.raw_model_output = bestPartialOutput;
    } else if (!parseErr.raw_model_output && raw) {
      parseErr.raw_model_output = raw;
    }
    throw parseErr;
  }

  const yoloLines = [];
  let droppedUnknownClass = 0;
  let droppedInvalidBox = 0;

  for (let i = 0; i < detections.length; i += 1) {
    const det = detections[i];
    const className = String(det && det.class_name !== undefined ? det.class_name : "").trim();
    if (!className) {
      droppedUnknownClass += 1;
      continue;
    }
    const classId = classMap[className.toLowerCase()];
    if (classId === undefined) {
      droppedUnknownClass += 1;
      continue;
    }

    const bbox = extractBBox(det);
    if (!bbox) {
      droppedInvalidBox += 1;
      continue;
    }

    let yolo = null;
    try {
      yolo = bboxToYolo(bbox);
    } catch (_err) {
      droppedInvalidBox += 1;
      continue;
    }
    if (!yolo) {
      droppedInvalidBox += 1;
      continue;
    }

    yoloLines.push(`${classId} ${fmt6(yolo[0])} ${fmt6(yolo[1])} ${fmt6(yolo[2])} ${fmt6(yolo[3])}`);
  }

  const { labelsPath, rawPath } = getArtifactPaths(job, labelsRoot, rawRoot);

  ensureParentDir(labelsPath);
  ensureParentDir(rawPath);

  const labelBody = yoloLines.length ? `${yoloLines.join("\n")}\n` : "";
  fs.write_text(labelsPath, labelBody);

  writeJsonFile(rawPath, {
    split: job.split,
    image_path: job.imagePath,
    relative_path: job.relativePath,
    classes_count: classes.length,
    raw_model_output: raw,
    parsed,
    stats: {
      input_detections: detections.length,
      written_detections: yoloLines.length,
      dropped_unknown_class: droppedUnknownClass,
      dropped_invalid_box: droppedInvalidBox
    }
  });

  return {
    labelsPath,
    rawPath,
    written: yoloLines.length,
    droppedUnknownClass,
    droppedInvalidBox
  };
}

function buildJobs(datasetRoot, splits) {
  const imagesRoot = joinPath(datasetRoot, "images");
  const jobs = [];
  const perSplitCounts = {};

  for (let i = 0; i < splits.length; i += 1) {
    const split = splits[i];
    const splitDir = joinPath(imagesRoot, split);
    const imagePaths = listSplitImages(splitDir);
    perSplitCounts[split] = imagePaths.length;

    for (let j = 0; j < imagePaths.length; j += 1) {
      const imagePath = imagePaths[j];
      jobs.push({
        split,
        splitDir,
        imagePath,
        relativePath: toRelativePath(imagePath, splitDir)
      });
    }
  }

  return { jobs, perSplitCounts };
}

function initRunLog(metaDir) {
  runtime.logPath = joinPath(metaDir, LOG_FILENAME);
  runtime.logWriteFailed = false;
  fs.append_text(runtime.logPath, `\n=== ${AGENT_NAME} run ${nowIso()} ===\n`);
  logInfo(`Run log file: ${runtime.logPath}`);
}

export default async function main(input) {
  const runOptions = parseRunOptions(input);
  let stage = "init";
  let datasetRoot = "";
  let summary = null;

  try {
    stage = "resolve_cwd";
    datasetRoot = fs.cwd();

    stage = "build_paths";
    const imagesRoot = joinPath(datasetRoot, "images");
    const trainDir = joinPath(imagesRoot, "train");
    const valDir = joinPath(imagesRoot, "val");
    const testDir = joinPath(imagesRoot, "test");
    const metaDir = joinPath(datasetRoot, "meta");
    const classesPath = joinPath(metaDir, "classes.txt");

    stage = "validate_layout";
    assertExistsDir(imagesRoot, "images root");
    assertExistsDir(trainDir, "train split");
    assertExistsDir(metaDir, "meta root");
    assertExistsFile(classesPath, "classes file");

    stage = "init_logging";
    initRunLog(metaDir);
    logInfo(`Dataset root (cwd): ${datasetRoot}`);
    logInfo(
      `Run options: resume=${runOptions.resume} retryFailedOnly=${runOptions.retryFailedOnly} listFailed=${runOptions.listFailedKind || "off"}`
    );
    if (runOptions.unknownFlags.length > 0) {
      logWarn(`Unknown flags ignored: ${runOptions.unknownFlags.join(", ")}`);
    }
    logInfo("Required dataset structure validation passed");

    stage = "parse_classes";
    const classes = parseClasses(classesPath);
    const classMap = classIndexMap(classes);

    stage = "resolve_splits";
    const splits = ["train"];
    if (fs.exists(valDir) && fs.is_dir(valDir)) {
      splits.push("val");
      logInfo("Detected optional split: val");
    } else {
      logInfo("Optional split not found: images/val");
    }
    if (fs.exists(testDir) && fs.is_dir(testDir)) {
      splits.push("test");
      logInfo("Detected optional split: test");
    } else {
      logInfo("Optional split not found: images/test");
    }

    stage = "ensure_output_dirs";
    const labelsRoot = joinPath(datasetRoot, "labels");
    const rawRoot = joinPath(metaDir, "raw");
    ensureDir(labelsRoot);
    ensureDir(rawRoot);
    for (let i = 0; i < splits.length; i += 1) {
      ensureDir(joinPath(labelsRoot, splits[i]));
      ensureDir(joinPath(rawRoot, splits[i]));
    }

    stage = "scan_images";
    const scan = buildJobs(datasetRoot, splits);
    const jobs = scan.jobs;
    const perSplitCounts = scan.perSplitCounts;
    const totalImages = jobs.length;
    logInfo(`Total images discovered: ${totalImages}`);

    stage = "list_failed";
    if (runOptions.listFailedKind) {
      let failedCount = 0;
      logInfo(`Listing failed files mode='${runOptions.listFailedKind}'`);
      for (let i = 0; i < jobs.length; i += 1) {
        const job = jobs[i];
        const state = inspectJobState(job, labelsRoot, rawRoot);
        if (state.status !== "failed") continue;
        failedCount += 1;

        if (runOptions.listFailedKind === "image") {
          print(job.imagePath);
        } else if (runOptions.listFailedKind === "raw") {
          print(state.paths.rawPath);
        } else if (runOptions.listFailedKind === "label") {
          print(state.paths.labelsPath);
        } else {
          print(
            `${job.imagePath}\t${state.paths.labelsPath}\t${state.paths.rawPath}\t${state.reason}`
          );
        }
      }

      logInfo(`Failed files listed: ${failedCount}`);
      set_result({
        ok: true,
        phase: "list_failed",
        stage,
        dataset_root: datasetRoot,
        mode: runOptions.listFailedKind,
        total: totalImages,
        failed: failedCount,
        run_options: {
          resume: runOptions.resume,
          retry_failed_only: runOptions.retryFailedOnly,
          list_failed: runOptions.listFailedKind,
          unknown_flags: runOptions.unknownFlags
        },
        output: {
          labels_root: labelsRoot,
          raw_root: rawRoot,
          log_file: runtime.logPath
        }
      });
      return;
    }

    stage = "build_prompt";
    const promptText = buildDetectionPrompt(classes);

    stage = "select_jobs";
    const selectedJobs = [];
    const stateCounts = { done: 0, failed: 0, new: 0 };
    let skippedDone = 0;
    let skippedNotFailed = 0;

    for (let i = 0; i < jobs.length; i += 1) {
      const job = jobs[i];
      const state = inspectJobState(job, labelsRoot, rawRoot);
      stateCounts[state.status] = (stateCounts[state.status] || 0) + 1;

      if (runOptions.retryFailedOnly) {
        if (state.status === "failed") selectedJobs.push(job);
        else skippedNotFailed += 1;
        continue;
      }

      if (runOptions.resume) {
        if (state.status === "done") {
          skippedDone += 1;
        } else {
          selectedJobs.push(job);
        }
        continue;
      }

      selectedJobs.push(job);
    }

    logInfo(
      `Job selection: discovered=${jobs.length} selected=${selectedJobs.length} done=${stateCounts.done} failed=${stateCounts.failed} new=${stateCounts.new}`
    );
    if (runOptions.resume) {
      logInfo(`Resume mode: skipped_done=${skippedDone}`);
    }
    if (runOptions.retryFailedOnly) {
      logInfo(`Retry-failed-only mode: skipped_not_failed=${skippedNotFailed}`);
    }

    stage = "annotate";
    let success = 0;
    let failed = 0;
    let empty = 0;
    let droppedUnknownClass = 0;
    let droppedInvalidBox = 0;
    const perSplitStats = {};
    for (let i = 0; i < splits.length; i += 1) {
      perSplitStats[splits[i]] = {
        total: perSplitCounts[splits[i]] || 0,
        success: 0,
        failed: 0,
        empty: 0
      };
    }

    for (let i = 0; i < selectedJobs.length; i += 1) {
      const job = selectedJobs[i];
      logInfo(`[${i + 1}/${selectedJobs.length}] split=${job.split} image=${job.relativePath}`);
      try {
        const res = await annotateImage(job, classes, classMap, labelsRoot, rawRoot, promptText);
        success += 1;
        perSplitStats[job.split].success += 1;
        droppedUnknownClass += res.droppedUnknownClass;
        droppedInvalidBox += res.droppedInvalidBox;
        if (res.written === 0) {
          empty += 1;
          perSplitStats[job.split].empty += 1;
          logWarn(`No detections written for ${job.split}/${job.relativePath}`);
        } else {
          logInfo(
            `Wrote ${res.written} detections -> ${res.labelsPath} (raw: ${res.rawPath})`
          );
        }
      } catch (e) {
        const err = errorToObject(e);
        failed += 1;
        perSplitStats[job.split].failed += 1;

        const { labelsPath, rawPath } = getArtifactPaths(job, labelsRoot, rawRoot);
        ensureParentDir(labelsPath);
        ensureParentDir(rawPath);

        fs.write_text(labelsPath, "");
        writeJsonFile(rawPath, {
          split: job.split,
          image_path: job.imagePath,
          relative_path: job.relativePath,
          error: err.message,
          stack: err.stack,
          raw_model_output: err.raw_model_output
        });

        logError(
          `Failed image split=${job.split} image=${job.relativePath}: ${err.message}`
        );
        if (err.raw_model_output) {
          const preview = err.raw_model_output.length > 800
            ? `${err.raw_model_output.slice(0, 800)}...`
            : err.raw_model_output;
          logError(`Failure raw output preview (${err.raw_model_output.length} chars): ${preview}`);
        }
      }
    }

    stage = "complete";
    summary = {
      ok: true,
      phase: "annotation",
      stage,
      dataset_root: datasetRoot,
      splits,
      classes_count: classes.length,
      total: totalImages,
      queued: selectedJobs.length,
      success,
      failed,
      empty,
      skipped_done: skippedDone,
      skipped_not_failed: skippedNotFailed,
      existing_state: stateCounts,
      dropped_unknown_class: droppedUnknownClass,
      dropped_invalid_box: droppedInvalidBox,
      per_split: perSplitStats,
      run_options: {
        resume: runOptions.resume,
        retry_failed_only: runOptions.retryFailedOnly,
        list_failed: runOptions.listFailedKind,
        unknown_flags: runOptions.unknownFlags
      },
      output: {
        labels_root: labelsRoot,
        raw_root: rawRoot,
        log_file: runtime.logPath
      }
    };

    logInfo(
      `Run complete: total=${summary.total} success=${summary.success} failed=${summary.failed} empty=${summary.empty}`
    );
    set_result(summary);
  } catch (e) {
    const err = errorToObject(e);
    logError(`Failure at stage='${stage}': ${err.message}`);
    if (err.stack) logError(err.stack);
    set_result({
      ok: false,
      phase: "annotation",
      stage,
      dataset_root: datasetRoot || null,
      run_options: {
        resume: runOptions.resume,
        retry_failed_only: runOptions.retryFailedOnly,
        list_failed: runOptions.listFailedKind,
        unknown_flags: runOptions.unknownFlags
      },
      error: err.message,
      stack: err.stack,
      log_file: runtime.logPath || null
    });
  }
}
