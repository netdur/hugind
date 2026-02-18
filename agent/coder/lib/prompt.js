export function buildPrompt(params) {
  const taskText = params.taskText;
  const issueText = params.issueText;
  const knownFiles = params.knownFiles;
  const priorErrors = params.priorErrors;
  const projectRoot = params.projectRoot;
  const history = params.history || [];
  const projectTreeProfile = params.projectTreeProfile || "";
  const iteration = params.iteration || 1;
  const maxTurns = params.maxTurns || iteration;
  const limits = params.limits;

  const fileBlocks = knownFiles.map((f) => {
    return [
      `FILE: ${f.relPath}`,
      "```",
      f.content,
      "```"
    ].join("\n");
  }).join("\n\n");

  const errorBlock = priorErrors.length
    ? `Previous issues:\n- ${priorErrors.join("\n- ")}`
    : "Previous issues: none";

  const historyBlock = history.length
    ? history.map((h, idx) => `${idx + 1}. ${h}`).join("\n")
    : "(none)";

  return [
    "You are a local coding agent.",
    `Iteration: ${iteration}/${maxTurns}`,
    "Return ONLY a JSON object with one of two actions:",
    "1) request_context",
    "2) propose_patch",
    "",
    "Strict schema:",
    "{",
    "  \"action\": \"request_context\" | \"propose_patch\",",
    "  \"reason\": string,",
    "  \"needed_paths\": string[],",
    "  \"edits\": [{\"path\": string, \"content\": string}]",
    "}",
    "",
    "Rules:",
    "- For request_context: provide needed_paths only, leave edits empty.",
    "- For propose_patch: provide edits only, each edit is full file content.",
    "- Do not include any extra keys.",
    `- All paths in needed_paths/edits.path are relative to project root: ${projectRoot}.`,
    "- Your RESPONSE must not include markdown/code fences.",
    "- Keep total proposed content within max_patch_chars.",
    "",
    `Limits: max_files=${limits.maxFiles}, max_patch_chars=${limits.maxPatchChars}`,
    "",
    "Task markdown:",
    "```md",
    taskText,
    "```",
    "",
    "Issue markdown:",
    "```md",
    issueText || "(none)",
    "```",
    "",
    errorBlock,
    "",
    "Interaction history:",
    historyBlock,
    "",
    projectTreeProfile ? "Project tree profile:" : "",
    projectTreeProfile ? "```" : "",
    projectTreeProfile || "",
    projectTreeProfile ? "```" : "",
    projectTreeProfile ? "" : "",
    "Known project files:",
    fileBlocks || "(none loaded yet)",
    "",
    "Now respond with JSON object only."
  ].join("\n");
}

export function buildCorrectionPrompt(params) {
  const taskText = params.taskText;
  const issueText = params.issueText;
  const projectRoot = params.projectRoot;
  const currentPatch = params.currentPatch;
  const maxPatchChars = params.maxPatchChars;

  return [
    "You are a local coding agent in correction phase.",
    "A first patch already exists. Review it against task + issue and either keep it or revise it.",
    "",
    "Return ONLY a JSON object with one of two actions:",
    "1) keep_patch",
    "2) propose_patch",
    "",
    "Strict schema:",
    "{",
    "  \"action\": \"keep_patch\" | \"propose_patch\",",
    "  \"reason\": string,",
    "  \"edits\": [{\"path\": string, \"content\": string}]",
    "}",
    "",
    "Rules:",
    "- If current patch is already correct, return action=keep_patch and edits=[].",
    "- If fixes are needed, return action=propose_patch with full-file contents.",
    "- Do not include any extra keys.",
    `- All edits.path values are relative to project root: ${projectRoot}.`,
    "- Your RESPONSE must not include markdown/code fences.",
    `- Keep total proposed content within max_patch_chars=${maxPatchChars}.`,
    "",
    "Task markdown:",
    "```md",
    taskText,
    "```",
    "",
    "Issue markdown:",
    "```md",
    issueText || "(none)",
    "```",
    "",
    "Current patch (unified diff):",
    "```diff",
    currentPatch || "(empty)",
    "```",
    "",
    "Now respond with JSON object only."
  ].join("\n");
}
