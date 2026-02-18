import { toInt } from "./common.js";

export function parseCliArgs(rawArgs) {
  const args = Array.isArray(rawArgs) ? rawArgs.slice() : [];
  if (args[0] === "--") args.shift();

  const options = {
    task: "",
    issue: "",
    output: "",
    project: ".",
    cwd: "",
    checks: [],
    maxIters: 3,
    maxFiles: 10,
    maxPatchChars: 120000,
    debugLlm: false,
    help: false
  };
  const errors = [];
  const seen = {};

  function setSingle(key, value, flag) {
    if (seen[key]) {
      errors.push(`duplicate flag: ${flag}`);
      return;
    }
    seen[key] = true;
    options[key] = String(value || "");
  }

  let i = 0;
  while (i < args.length) {
    const token = String(args[i] || "");

    if (token === "--help" || token === "-h") {
      options.help = true;
      i += 1;
      continue;
    }
    if (token === "--debug-llm") {
      options.debugLlm = true;
      i += 1;
      continue;
    }

    if (token === "--task" || token === "--issue" || token === "--output" || token === "--project" || token === "--cwd") {
      const value = args[i + 1];
      if (value === undefined || String(value).startsWith("--")) {
        errors.push(`missing value for ${token}`);
        i += 1;
        continue;
      }
      if (token === "--task") setSingle("task", value, token);
      if (token === "--issue") setSingle("issue", value, token);
      if (token === "--output") setSingle("output", value, token);
      if (token === "--project") setSingle("project", value, token);
      if (token === "--cwd") setSingle("cwd", value, token);
      i += 2;
      continue;
    }

    if (token === "--check") {
      const value = args[i + 1];
      if (value === undefined || String(value).startsWith("--")) {
        errors.push("missing value for --check");
        i += 1;
        continue;
      }
      options.checks.push(String(value));
      i += 2;
      continue;
    }

    if (token === "--checks") {
      const value = args[i + 1];
      if (value === undefined || String(value).startsWith("--")) {
        errors.push("missing value for --checks");
        i += 1;
        continue;
      }
      options.checks = options.checks.concat(String(value).split(",").map((v) => v.trim()).filter(Boolean));
      i += 2;
      continue;
    }

    if (token === "--max-iters" || token === "--max-files" || token === "--max-patch-chars") {
      const value = args[i + 1];
      if (value === undefined || String(value).startsWith("--")) {
        errors.push(`missing value for ${token}`);
        i += 1;
        continue;
      }
      if (token === "--max-iters") options.maxIters = toInt(value, options.maxIters);
      if (token === "--max-files") options.maxFiles = toInt(value, options.maxFiles);
      if (token === "--max-patch-chars") options.maxPatchChars = toInt(value, options.maxPatchChars);
      i += 2;
      continue;
    }

    errors.push(`unknown flag: ${token}`);
    i += 1;
  }

  if (!options.task) errors.push("missing required flag: --task");
  if (!options.output) errors.push("missing required flag: --output");

  if (options.maxIters < 1) options.maxIters = 1;
  if (options.maxFiles < 1) options.maxFiles = 1;
  if (options.maxPatchChars < 1024) options.maxPatchChars = 1024;

  return { options, errors };
}
