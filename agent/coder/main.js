import { usage, finish, unique, safeReadText } from "./lib/common.js";
import { parseCliArgs } from "./lib/cli.js";
import { llmJson } from "./lib/llm.js";
import { formatUnifiedDiffFile } from "./lib/diff.js";
import { buildPrompt, buildCorrectionPrompt } from "./lib/prompt.js";
import { buildProjectTreeProfile } from "./lib/tree_profile.js";
import {
  normalizePath,
  joinPath,
  dirname,
  toRepoRelative,
  resolveModelPath
} from "./lib/path_utils.js";

export default async function main(input) {
  const audit = {
    files_read: [],
    files_written: [],
    commands_run: [],
    guardrail_violations: []
  };

  const result = {
    status: "failed",
    summary: "",
    iterations: 0,
    files_changed: [],
    output_diff_path: "",
    checks: [],
    errors: []
  };

  function noteRead(path) {
    audit.files_read.push(path);
    audit.files_read = unique(audit.files_read);
  }

  function noteWrite(path) {
    audit.files_written.push(path);
    audit.files_written = unique(audit.files_written);
  }

  function violation(msg) {
    audit.guardrail_violations.push(msg);
    audit.guardrail_violations = unique(audit.guardrail_violations);
  }

  function validateProposedEdits(rawEdits, projectRoot, cwd, outputPath, opts, priorErrors) {
    if (!Array.isArray(rawEdits) || rawEdits.length === 0) {
      priorErrors.push("propose_patch requires non-empty edits");
      return { ok: false, edits: [] };
    }

    let patchChars = 0;
    const normalizedEdits = [];

    for (let i = 0; i < rawEdits.length; i += 1) {
      const item = rawEdits[i] || {};
      const rawPath = String(item.path || "").trim();
      const content = String(item.content || "");

      if (!rawPath) {
        priorErrors.push("edit path cannot be empty");
        return { ok: false, edits: [] };
      }

      const resolved = resolveModelPath(projectRoot, cwd, rawPath, { requireExistingFile: false });
      if (!resolved.ok) {
        const msg = `edit outside project rejected: ${rawPath}`;
        violation(msg);
        priorErrors.push(msg);
        return { ok: false, edits: [] };
      }
      const absPath = resolved.absPath;

      if (normalizePath(absPath) === normalizePath(outputPath)) {
        const msg = `edit cannot target output diff path: ${rawPath}`;
        violation(msg);
        priorErrors.push(msg);
        return { ok: false, edits: [] };
      }

      patchChars += content.length;
      if (patchChars > opts.maxPatchChars) {
        const msg = `max_patch_chars exceeded (${opts.maxPatchChars})`;
        violation(msg);
        priorErrors.push(msg);
        return { ok: false, edits: [] };
      }

      normalizedEdits.push({
        absPath,
        relPath: toRepoRelative(cwd, absPath),
        content
      });
    }

    return { ok: true, edits: normalizedEdits };
  }

  function buildDiffFromEdits(edits) {
    const diffParts = [];
    const changedFiles = [];

    for (let i = 0; i < edits.length; i += 1) {
      const edit = edits[i];
      const oldExists = fs.exists(edit.absPath) && fs.is_file(edit.absPath);
      const oldText = oldExists ? safeReadText(edit.absPath) : "";
      if (oldExists) noteRead(edit.absPath);

      const newText = edit.content;
      const fileDiff = formatUnifiedDiffFile(edit.relPath, oldText, newText, oldExists, true);
      if (!fileDiff) continue;

      diffParts.push(fileDiff);
      changedFiles.push(edit.relPath);
    }

    return {
      fullDiff: diffParts.join("\n"),
      changedFiles
    };
  }

  try {
    const sessionMeta = input && input.meta && input.meta.session ? input.meta.session : null;
    if (sessionMeta) {
      const mode = String(sessionMeta.mode || "");
      const id = String(sessionMeta.id || "");
      print(`[coder] session.mode=${mode || "(none)"}`);
      print(`[coder] session.id=${id || "(none)"}`);
    } else {
      print("[coder] session.meta=(none)");
    }

    const argv = (input && Array.isArray(input.args)) ? input.args : [];
    const parsed = parseCliArgs(argv);
    const opts = parsed.options;

    if (opts.help) {
      usage();
      result.status = "needs_input";
      result.summary = "Help requested";
      result.errors = parsed.errors;
      result.audit = audit;
      return finish(result);
    }

    if (parsed.errors.length > 0) {
      usage();
      result.status = "needs_input";
      result.summary = "Invalid CLI arguments";
      result.errors = parsed.errors;
      result.audit = audit;
      return finish(result);
    }

    print("[coder] input validated");

    const hostCwd = normalizePath(fs.realpath(fs.cwd()));
    const cwd = opts.cwd ? joinPath(hostCwd, opts.cwd) : hostCwd;
    const taskPath = joinPath(cwd, opts.task);
    const issuePath = opts.issue ? joinPath(cwd, opts.issue) : "";
    const outputPath = joinPath(cwd, opts.output);
    const projectRoot = joinPath(cwd, opts.project || ".");
    result.output_diff_path = outputPath;

    print(`[coder] host_cwd=${hostCwd}`);
    print(`[coder] cwd=${cwd}`);
    print(`[coder] task=${taskPath}`);
    if (issuePath) print(`[coder] issue=${issuePath}`);
    print(`[coder] output=${outputPath}`);
    print(`[coder] project=${projectRoot}`);

    if (!fs.exists(cwd) || !fs.is_dir(cwd)) {
      result.status = "failed";
      result.summary = "CWD path not found";
      result.errors.push(`cwd path missing or not dir: ${cwd}`);
      result.audit = audit;
      return finish(result);
    }

    if (!fs.exists(projectRoot) || !fs.is_dir(projectRoot)) {
      result.status = "failed";
      result.summary = "Project path not found";
      result.errors.push(`project path missing or not dir: ${projectRoot}`);
      result.audit = audit;
      return finish(result);
    }

    if (!fs.exists(taskPath) || !fs.is_file(taskPath)) {
      result.status = "failed";
      result.summary = "Task file not found";
      result.errors.push(`task file missing: ${taskPath}`);
      result.audit = audit;
      return finish(result);
    }

    if (issuePath && (!fs.exists(issuePath) || !fs.is_file(issuePath))) {
      result.status = "failed";
      result.summary = "Issue file not found";
      result.errors.push(`issue file missing: ${issuePath}`);
      result.audit = audit;
      return finish(result);
    }

    noteRead(taskPath);
    const taskText = safeReadText(taskPath);

    let issueText = "";
    if (issuePath) {
      noteRead(issuePath);
      issueText = safeReadText(issuePath);
    }

    const outputDir = dirname(outputPath);
    if (!fs.exists(outputDir)) {
      fs.mkdir(outputDir, true);
    }

    fs.write_text(outputPath, "");
    noteWrite(outputPath);
    print("[coder] output diff cleared");

    const knownFileMap = {};
    const knownFiles = [];
    const projectTreeProfile = buildProjectTreeProfile(projectRoot, 4, 300);
    print("[coder] project tree profile captured");

    const priorErrors = [];
    const history = [];
    let edits = null;

    const maxTurns = Math.max(3, opts.maxIters * 3);
    let turn = 0;

    print("[coder] context collected");

    while (turn < maxTurns) {
      turn += 1;
      print(`[coder] llm iteration ${turn}/${maxTurns}`);

      const prompt = buildPrompt({
        taskText,
        issueText,
        knownFiles,
        priorErrors,
        projectRoot: toRepoRelative(cwd, projectRoot),
        projectTreeProfile: turn === 1 ? projectTreeProfile : "",
        history: history.slice(-20),
        iteration: turn,
        maxTurns,
        limits: {
          maxFiles: opts.maxFiles,
          maxPatchChars: opts.maxPatchChars
        }
      });

      let reply;
      try {
        if (opts.debugLlm) {
          print("[coder] ---- prompt begin ----");
          print(prompt);
          print("[coder] ---- prompt end ----");
        }

        const llmRes = await llmJson(prompt, 1);
        if (opts.debugLlm) {
          if (llmRes.usedFixup) {
            print(`[coder] first response parse error: ${llmRes.firstParseError}`);
            print("[coder] ---- first (invalid) model response begin ----");
            print(llmRes.firstRaw);
            print("[coder] ---- first (invalid) model response end ----");
            print("[coder] ---- fixup model response begin ----");
            print(llmRes.fixedRaw);
            print("[coder] ---- fixup model response end ----");
          } else {
            print("[coder] ---- model response begin ----");
            print(llmRes.firstRaw);
            print("[coder] ---- model response end ----");
          }
        }
        reply = llmRes.data;
      } catch (e) {
        result.errors.push(`llm parse failure: ${String(e)}`);
        priorErrors.push(`Invalid JSON response from model on turn ${turn}`);
        break;
      }

      const action = String(reply.action || "");
      const reason = String(reply.reason || "");

      if (action === "request_context") {
        const neededPaths = Array.isArray(reply.needed_paths) ? reply.needed_paths : [];
        history.push(`turn ${turn}: action=request_context reason=${reason || "(none)"} needed_paths=${neededPaths.join(", ") || "(none)"}`);
        if (neededPaths.length === 0) {
          priorErrors.push("request_context requires non-empty needed_paths");
          history.push(`turn ${turn}: result=invalid_request_context_empty_paths`);
          continue;
        }

        let loadedNow = 0;
        for (let i = 0; i < neededPaths.length; i += 1) {
          const rawPath = String(neededPaths[i] || "").trim();
          if (!rawPath) continue;

          const resolved = resolveModelPath(projectRoot, cwd, rawPath, { requireExistingFile: true });
          if (!resolved.ok) {
            const msg = `context path outside project rejected: ${rawPath}`;
            violation(msg);
            priorErrors.push(msg);
            continue;
          }
          const absPath = resolved.absPath;

          if (!fs.exists(absPath) || !fs.is_file(absPath)) {
            priorErrors.push(`context path missing or not file: ${rawPath}`);
            continue;
          }

          const relPath = toRepoRelative(cwd, absPath);
          if (knownFileMap[relPath]) continue;

          if (knownFiles.length >= opts.maxFiles) {
            const msg = `max_files limit reached (${opts.maxFiles})`;
            violation(msg);
            priorErrors.push(msg);
            break;
          }

          const content = safeReadText(absPath);
          noteRead(absPath);

          knownFileMap[relPath] = true;
          knownFiles.push({ relPath, content });
          loadedNow += 1;
        }

        if (loadedNow === 0) {
          priorErrors.push("No additional context loaded");
          history.push(`turn ${turn}: result=context_loaded=0`);
        } else {
          history.push(`turn ${turn}: result=context_loaded=${loadedNow}`);
        }

        continue;
      }

      if (action === "propose_patch") {
        const rawEdits = Array.isArray(reply.edits) ? reply.edits : [];
        history.push(`turn ${turn}: action=propose_patch reason=${reason || "(none)"} edits=${rawEdits.length}`);
        const validated = validateProposedEdits(rawEdits, projectRoot, cwd, outputPath, opts, priorErrors);
        if (!validated.ok) {
          history.push(`turn ${turn}: result=invalid_patch`);
          continue;
        }

        edits = validated.edits;
        result.iterations += 1;
        history.push(`turn ${turn}: result=patch_valid`);
        print("[coder] patch proposed and validated");
        break;
      }

      priorErrors.push(`unknown action: ${action}`);
      history.push(`turn ${turn}: result=unknown_action_${action || "(empty)"}`);
    }

    if (!edits || edits.length === 0) {
      result.status = "failed";
      result.summary = "Model did not produce a valid patch";
      result.errors = result.errors.concat(priorErrors.slice(-5));
      result.audit = audit;
      return finish(result);
    }

    let finalEdits = edits;
    let computed = buildDiffFromEdits(finalEdits);

    if (issueText && issueText.trim().length > 0) {
      print("[coder] correction phase start");
      const correctionPrompt = buildCorrectionPrompt({
        taskText,
        issueText,
        projectRoot: toRepoRelative(cwd, projectRoot),
        currentPatch: computed.fullDiff,
        maxPatchChars: opts.maxPatchChars
      });

      try {
        if (opts.debugLlm) {
          print("[coder] ---- correction prompt begin ----");
          print(correctionPrompt);
          print("[coder] ---- correction prompt end ----");
        }
        const corrRes = await llmJson(correctionPrompt, 1);
        if (opts.debugLlm) {
          if (corrRes.usedFixup) {
            print(`[coder] correction first response parse error: ${corrRes.firstParseError}`);
            print("[coder] ---- correction first (invalid) response begin ----");
            print(corrRes.firstRaw);
            print("[coder] ---- correction first (invalid) response end ----");
            print("[coder] ---- correction fixup response begin ----");
            print(corrRes.fixedRaw);
            print("[coder] ---- correction fixup response end ----");
          } else {
            print("[coder] ---- correction response begin ----");
            print(corrRes.firstRaw);
            print("[coder] ---- correction response end ----");
          }
        }

        const corr = corrRes.data;
        const corrAction = String(corr.action || "");
        if (corrAction === "propose_patch") {
          const corrRawEdits = Array.isArray(corr.edits) ? corr.edits : [];
          const corrValidated = validateProposedEdits(corrRawEdits, projectRoot, cwd, outputPath, opts, result.errors);
          if (corrValidated.ok) {
            finalEdits = corrValidated.edits;
            computed = buildDiffFromEdits(finalEdits);
            print("[coder] correction patch accepted");
          } else {
            print("[coder] correction patch rejected; keeping initial patch");
          }
        } else {
          print("[coder] correction kept initial patch");
        }
      } catch (e) {
        result.errors.push(`correction phase failed: ${String(e)}`);
        print("[coder] correction failed; keeping initial patch");
      }
    }

    const fullDiff = computed.fullDiff;
    fs.write_text(outputPath, fullDiff);
    noteWrite(outputPath);

    print("[coder] diff generated");

    result.status = "success";
    result.summary = computed.changedFiles.length
      ? `Generated diff for ${computed.changedFiles.length} file(s)`
      : "No file changes required";
    result.files_changed = computed.changedFiles;

    if (opts.checks.length > 0) {
      result.checks.push({
        command: opts.checks.join(" && "),
        ok: false,
        output_excerpt: "Checks were provided but verify mode is not implemented in this version."
      });
    }

    result.audit = audit;
    return finish(result);
  } catch (e) {
    result.status = "failed";
    result.summary = "Unhandled error";
    result.errors.push(String(e));
    result.audit = audit;
    return finish(result);
  }
}
