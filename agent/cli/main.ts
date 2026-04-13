// @ts-nocheck
import { print, printRaw, input, llmChatStream, runCommand, getArgsJson, alloc } from "./wasm_sdk";
import { JSON } from "assemblyscript-json/assembly";
import { ParsedResponse, extractJson, parseJsonResponse, parseResponse } from "./lib/json";
import { isIncompleteCommand, looksUnsafe, shouldExit } from "./lib/safety";
import {
  buildCancelPrompt,
  buildFollowupPrompt,
  buildInitialPrompt,
  buildInvalidCommandPrompt,
  escapeJsonString,
} from "./lib/prompt";
import { smartTruncate } from "./lib/truncate";

export { alloc };

// -----------------------------
// LLM request options
// -----------------------------
const LLM_MAX_TOKENS: i32 = 4096;
const LLM_TEMPERATURE: f32 = 0.2;
const LLM_TOP_P: f32 = 0.95;
const LLM_ENABLE_THINKING: bool = true;
const LLM_THINKING_BUDGET_TOKENS: i32 = 1024;
const LLM_STREAM_VISIBLE_OUTPUT: bool = true;
const LLM_FORCE_JSON_RESPONSE_FORMAT: bool = true;
const LLM_FALLBACK_THINKING_SPINNER: bool = true;

// Thinking tag pairs: [open, close] for each supported model family.
const THINK_OPEN_TAGS: string[] = ["<think>", "<|channel>thought"];
const THINK_CLOSE_TAGS: string[] = ["</think>", "<channel|>"];
const THINK_SPINNER_FRAMES = "|/-\\";

// Computed max tag length for scan-tail buffer sizing.
function maxTagLength(): i32 {
  let m: i32 = 0;
  for (let i = 0; i < THINK_OPEN_TAGS.length; i++) {
    if (THINK_OPEN_TAGS[i].length > m) m = THINK_OPEN_TAGS[i].length;
  }
  for (let i = 0; i < THINK_CLOSE_TAGS.length; i++) {
    if (THINK_CLOSE_TAGS[i].length > m) m = THINK_CLOSE_TAGS[i].length;
  }
  return m;
}

// Find earliest occurrence of any tag from the array. Returns [index, tagIndex] or [-1, -1].
function findEarliestTag(text: string, tags: string[]): i32[] {
  let bestIdx: i32 = -1;
  let bestTag: i32 = -1;
  for (let i = 0; i < tags.length; i++) {
    const idx = text.indexOf(tags[i]);
    if (idx >= 0 && (bestIdx < 0 || idx < bestIdx)) {
      bestIdx = idx;
      bestTag = i;
    }
  }
  return [bestIdx, bestTag];
}

let thinkingStreamActive: bool = false;
let thinkingSpinnerVisible: bool = false;
let thinkingSpinnerStep: i32 = 0;
let thinkingScanTail: string = "";
let thinkingOpenSeen: bool = false;
let thinkingCloseSeen: bool = false;
let debugPromptIo: bool = false;
let debugRawStream: bool = false;
let streamOutputPrinted: bool = false;
let fallbackThinkingActive: bool = false;

function showThinkingSpinnerTick(): void {
  if (debugPromptIo || debugRawStream) return;
  const frame = THINK_SPINNER_FRAMES.charAt(thinkingSpinnerStep % THINK_SPINNER_FRAMES.length);
  thinkingSpinnerStep++;
  printRaw("\r" + frame);
  thinkingSpinnerVisible = true;
}

function clearThinkingSpinner(): void {
  if (thinkingSpinnerVisible) {
    printRaw("\r \r");
    thinkingSpinnerVisible = false;
  }
}

function resetThinkingStreamState(): void {
  thinkingStreamActive = false;
  thinkingSpinnerStep = 0;
  thinkingScanTail = "";
  thinkingOpenSeen = false;
  thinkingCloseSeen = false;
  streamOutputPrinted = false;
  fallbackThinkingActive = LLM_ENABLE_THINKING && LLM_FALLBACK_THINKING_SPINNER;
  clearThinkingSpinner();
  if (fallbackThinkingActive) {
    showThinkingSpinnerTick();
  }
}

function updateThinkingFromDelta(delta: string): void {
  if (!LLM_ENABLE_THINKING || delta.length == 0) return;

  let scan = thinkingScanTail + delta;
  // Some models emit only a closing tag. Treat it as a valid boundary and
  // stop fallback thinking animation immediately.
  if (!thinkingStreamActive && !thinkingOpenSeen) {
    const closeResult = findEarliestTag(scan, THINK_CLOSE_TAGS);
    const closeWithoutOpenIdx = closeResult[0];
    const closeTagIdx = closeResult[1];
    if (closeWithoutOpenIdx >= 0) {
      thinkingCloseSeen = true;
      fallbackThinkingActive = false;
      clearThinkingSpinner();
      scan = scan.substring(closeWithoutOpenIdx + THINK_CLOSE_TAGS[closeTagIdx].length);
    }
  }

  if (!thinkingStreamActive) {
    const openResult = findEarliestTag(scan, THINK_OPEN_TAGS);
    const openIdx = openResult[0];
    const openTagIdx = openResult[1];
    if (openIdx >= 0) {
      thinkingStreamActive = true;
      thinkingOpenSeen = true;
      fallbackThinkingActive = false;
      thinkingSpinnerStep = 0;
      showThinkingSpinnerTick();
      scan = scan.substring(openIdx + THINK_OPEN_TAGS[openTagIdx].length);
    } else {
      const keep = maxTagLength() + 8;
      if (scan.length > keep) {
        thinkingScanTail = scan.substring(scan.length - keep);
      } else {
        thinkingScanTail = scan;
      }
      return;
    }
  }

  if (thinkingStreamActive) {
    const closeResult = findEarliestTag(scan, THINK_CLOSE_TAGS);
    const closeIdx = closeResult[0];
    const closeTagIdx = closeResult[1];
    if (closeIdx >= 0) {
      thinkingStreamActive = false;
      thinkingCloseSeen = true;
      clearThinkingSpinner();
      scan = scan.substring(closeIdx + THINK_CLOSE_TAGS[closeTagIdx].length);
    } else {
      showThinkingSpinnerTick();
    }
  }

  const keep = maxTagLength() + 16;
  if (scan.length > keep) {
    thinkingScanTail = scan.substring(scan.length - keep);
  } else {
    thinkingScanTail = scan;
  }
}

export function llm_on_token(ptr: i32, len: i32): void {
  if (len <= 0 || ptr < 0) return;
  const delta = String.UTF8.decodeUnsafe(ptr, len, false);
  const showThinkingDebug = debugPromptIo && LLM_ENABLE_THINKING;
  if (LLM_ENABLE_THINKING) {
    updateThinkingFromDelta(delta);
  }
  if (debugRawStream) return;
  if (!debugPromptIo) {
    // Normal mode: never stream raw model text (including JSON).
    // Keep only the spinner animation while thinking.
    if (fallbackThinkingActive && !thinkingOpenSeen) {
      showThinkingSpinnerTick();
    }
    return;
  }

  if (!LLM_STREAM_VISIBLE_OUTPUT) return;
  if (!showThinkingDebug && fallbackThinkingActive && !thinkingOpenSeen && !streamOutputPrinted) {
    showThinkingSpinnerTick();
  }
  if (!showThinkingDebug && LLM_ENABLE_THINKING && thinkingStreamActive) return;
  if (!showThinkingDebug && LLM_ENABLE_THINKING && thinkingOpenSeen && !thinkingCloseSeen) return;

  let visible = delta;
  if (LLM_ENABLE_THINKING && !showThinkingDebug) {
    for (let i = 0; i < THINK_OPEN_TAGS.length; i++) {
      while (visible.indexOf(THINK_OPEN_TAGS[i]) >= 0) {
        visible = visible.replace(THINK_OPEN_TAGS[i], "");
      }
    }
    for (let i = 0; i < THINK_CLOSE_TAGS.length; i++) {
      while (visible.indexOf(THINK_CLOSE_TAGS[i]) >= 0) {
        visible = visible.replace(THINK_CLOSE_TAGS[i], "");
      }
    }
  }
  if (visible.length > 0) {
    fallbackThinkingActive = false;
    clearThinkingSpinner();
    printRaw(visible);
    streamOutputPrinted = true;
  }
}

export function llm_on_sse(ptr: i32, len: i32): void {
  if (!debugRawStream) return;
  if (ptr < 0 || len < 0) return;
  const line = len > 0 ? String.UTF8.decodeUnsafe(ptr, len, false) : "";
  printRaw(line + "\n");
  streamOutputPrinted = true;
}

function hasDebugPromptIoFlag(flag: string): bool {
  const argsJson = getArgsJson();
  if (argsJson.length == 0) return false;

  const parsed = JSON.parse(argsJson);
  if (!parsed.isObj) return false;

  const root = changetype<JSON.Obj>(parsed);
  const argsArr = root.getArr("args");
  if (argsArr == null) return false;

  const items = (argsArr as JSON.Arr)._arr;
  for (let i = 0; i < items.length; i++) {
    const value = items[i];
    if (value != null && value.isString) {
      const arg = changetype<JSON.Str>(value)._str.trim().toLowerCase();
      if (arg == flag) return true;
    }
  }
  return false;
}

function parseDebugPromptIoFlag(): bool {
  return (
    hasDebugPromptIoFlag("--debug-prompt-io") ||
    hasDebugPromptIoFlag("--debug-prompts") ||
    hasDebugPromptIoFlag("--debug-llm")
  );
}

function parseDebugRawStreamFlag(): bool {
  return (
    hasDebugPromptIoFlag("--debug-raw-stream") ||
    hasDebugPromptIoFlag("--debug-sse")
  );
}

function buildLlmRequestJson(prompt: string): string {
  const escapedPrompt = escapeJsonString(prompt);
  let req =
    "{" +
    '"messages":[{"role":"user","content":"' + escapedPrompt + '"}],' +
    '"stream":true,' +
    '"max_tokens":' + LLM_MAX_TOKENS.toString() + "," +
    '"temperature":' + LLM_TEMPERATURE.toString() + "," +
    '"top_p":' + LLM_TOP_P.toString() + "," +
    '"enable_thinking":' + (LLM_ENABLE_THINKING ? "true" : "false");

  if (LLM_FORCE_JSON_RESPONSE_FORMAT) {
    req += ',"response_format":{"type":"json_object"}';
  }
  if (LLM_ENABLE_THINKING) {
    req += ',"thinking_budget_tokens":' + LLM_THINKING_BUDGET_TOKENS.toString();
  }
  req += "}";
  return req;
}

function normalizeLlmRaw(raw: string): string {
  if (!LLM_ENABLE_THINKING) return raw;

  // Find the last occurrence of any close tag and return text after it.
  let bestIdx: i32 = -1;
  let bestLen: i32 = 0;
  for (let i = 0; i < THINK_CLOSE_TAGS.length; i++) {
    const idx = raw.lastIndexOf(THINK_CLOSE_TAGS[i]);
    if (idx >= 0 && idx > bestIdx) {
      bestIdx = idx;
      bestLen = THINK_CLOSE_TAGS[i].length;
    }
  }
  if (bestIdx >= 0) {
    const after = raw.substring(bestIdx + bestLen).trim();
    if (after.length > 0) {
      return after;
    }
  }
  return raw;
}

function llmJsonWithRetries(basePrompt: string, maxRetries: i32): string {
  let lastRaw = "";
  for (let i = 0; i < maxRetries; i++) {
    let prompt = basePrompt;
    if (i > 0) {
      prompt +=
        "\n\nERROR: Last output was not valid JSON. Raw Output:\n" +
        lastRaw +
        "\n\nReturn a final valid JSON object.";
    }
    if (debugPromptIo) {
      print("[DEBUG] PROMPT >>>\n" + prompt + "\n<<< [DEBUG] PROMPT");
    }
    const requestJson = buildLlmRequestJson(prompt);
    resetThinkingStreamState();
    const streamRaw = llmChatStream(requestJson);
    clearThinkingSpinner();
    if (streamOutputPrinted) {
      printRaw("\n");
    }
    if (debugPromptIo && LLM_ENABLE_THINKING) {
      if (!thinkingOpenSeen && !thinkingCloseSeen) {
        print("[DEBUG] THINK: no visible think tags emitted.");
      } else if (!thinkingOpenSeen && thinkingCloseSeen) {
        print("[DEBUG] THINK: saw </think> without <think>.");
      }
    }
    if (debugPromptIo) {
      print("[DEBUG] RESPONSE >>>\n" + streamRaw + "\n<<< [DEBUG] RESPONSE");
    }
    const resp = normalizeLlmRaw(streamRaw);
    lastRaw = resp;

    const jsonText = extractJson(resp);
    if (jsonText.length == 0) continue;

    const value = JSON.parse(jsonText);
    if (value.isObj) return resp;
  }
  return lastRaw;
}

// -----------------------------
// Main
// -----------------------------
export function main(): void {
  debugPromptIo = parseDebugPromptIoFlag();
  debugRawStream = parseDebugRawStreamFlag();
  if (debugPromptIo && debugRawStream) {
    print("CLIv1 [debug-prompt-io, raw-stream]");
  } else if (debugPromptIo) {
    print("CLIv1 [debug-prompt-io]");
  } else if (debugRawStream) {
    print("CLIv1 [raw-stream]");
  } else {
    print("CLIv1");
  }

  // Probe OS
  const osShort = runCommand("uname -s").trim(); // "Darwin", "Linux", etc.

  while (true) {
    const user = input("> ").trim();
    if (user.length == 0) continue;
    if (shouldExit(user)) break;

    const maxSteps: i32 = 40;
    let response = llmJsonWithRetries(buildInitialPrompt(user, osShort), 10);
    let parsed = parseResponse(response);
    let finished = false;

    for (let step = 0; step < maxSteps; step++) {
      if (parsed.kind == "answer") {
        print(parsed.answer);
        finished = true;
        break;
      }

      if (parsed.kind == "command") {
        let cmd = parsed.command.trim();

        if (isIncompleteCommand(cmd)) {
          response = llmJsonWithRetries(buildInvalidCommandPrompt(cmd), 10);
          parsed = parseResponse(response);
          continue;
        }

        // Enforce confirm at runtime for unsafe/slow commands, regardless of LLM flag
        let needConfirm = parsed.confirm || looksUnsafe(cmd);

        if (needConfirm) {
          const decision = input("Run command? " + cmd + " (y/n) ").trim().toLowerCase();
          if (!(decision == "y" || decision == "yes")) {
            response = llmJsonWithRetries(buildCancelPrompt(cmd), 10);
            parsed = parseResponse(response);
            continue;
          }
        }

        // Execute
        print("RUN: " + cmd);
        const rawOut = runCommand(cmd);

        // Keep prompts bounded
        const outForPrompt = smartTruncate(rawOut, 4000);
        response = llmJsonWithRetries(buildFollowupPrompt(cmd, outForPrompt), 10);
        parsed = parseResponse(response);
        continue;
      }

      // Unknown shape: retry
      response = llmJsonWithRetries(
        "Return a valid JSON object matching the schema (kind/command/confirm/answer).",
        10
      );
      parsed = parseResponse(response);
    }

    if (!finished) {
      print("I could not finish this task within " + maxSteps.toString() + " steps.");
    }
  }
}
