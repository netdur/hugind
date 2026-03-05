// @ts-nocheck
import { print, printRaw, input, llmChatStream, runCommand, getArgsJson, alloc } from "./wasm_sdk";
import { JSON } from "assemblyscript-json/assembly";

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

const THINK_OPEN_TAG = "<think>";
const THINK_CLOSE_TAG = "</think>";
const THINK_SPINNER_FRAMES = "|/-\\";

class ParsedResponse {
  kind: string = "answer";
  command: string = "";
  confirm: bool = false;
  answer: string = "";
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

// -----------------------------
// JSON extraction/parsing
// -----------------------------
function extractJson(text: string): string {
  const trimmed = text.trim();
  if (trimmed.startsWith("{") && trimmed.endsWith("}")) return trimmed;

  // Allow fenced JSON, but don’t rely on it
  const startMarker = "```json";
  const endMarker = "```";
  const start = trimmed.indexOf(startMarker);
  if (start >= 0) {
    const after = start + startMarker.length;
    const end = trimmed.indexOf(endMarker, after);
    if (end > after) return trimmed.substring(after, end).trim();
  }

  // Fallback: first { .. last }
  const a = trimmed.indexOf("{");
  const b = trimmed.lastIndexOf("}");
  if (a >= 0 && b > a) return trimmed.substring(a, b + 1);

  return "";
}

function parseJsonResponse(text: string): ParsedResponse | null {
  const jsonText = extractJson(text);
  if (jsonText.length == 0) return null;

  const value = JSON.parse(jsonText);
  if (!value.isObj) return null;

  const obj = changetype<JSON.Obj>(value);

  const kindVal = obj.getString("kind");
  const cmdVal = obj.getString("command");
  const answerVal = obj.getString("answer");
  const confirmVal = obj.getBool("confirm");

  const kind = kindVal ? kindVal._str : "";
  const command = cmdVal ? cmdVal._str : "";
  const answer = answerVal ? answerVal._str : "";
  const confirm = confirmVal ? confirmVal._bool : false;

  // Normalize: if answer present, treat as answer; if command present, treat as command
  if (kind == "command" || command.length > 0) {
    return { kind: "command", command, confirm, answer: "" };
  }
  return { kind: "answer", command: "", confirm: false, answer };
}

function parseResponse(text: string): ParsedResponse {
  const jsonParsed = parseJsonResponse(text);
  if (jsonParsed != null) return jsonParsed as ParsedResponse;

  // Last-resort fallback: treat whole text as answer
  return { kind: "answer", command: "", confirm: false, answer: text.trim() };
}

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
    const closeWithoutOpenIdx = scan.indexOf(THINK_CLOSE_TAG);
    if (closeWithoutOpenIdx >= 0) {
      thinkingCloseSeen = true;
      fallbackThinkingActive = false;
      clearThinkingSpinner();
      scan = scan.substring(closeWithoutOpenIdx + THINK_CLOSE_TAG.length);
    }
  }

  if (!thinkingStreamActive) {
    const openIdx = scan.indexOf(THINK_OPEN_TAG);
    if (openIdx >= 0) {
      thinkingStreamActive = true;
      thinkingOpenSeen = true;
      fallbackThinkingActive = false;
      thinkingSpinnerStep = 0;
      showThinkingSpinnerTick();
      scan = scan.substring(openIdx + THINK_OPEN_TAG.length);
    } else {
      const keep = THINK_OPEN_TAG.length + 8;
      if (scan.length > keep) {
        thinkingScanTail = scan.substring(scan.length - keep);
      } else {
        thinkingScanTail = scan;
      }
      return;
    }
  }

  if (thinkingStreamActive) {
    const closeIdx = scan.indexOf(THINK_CLOSE_TAG);
    if (closeIdx >= 0) {
      thinkingStreamActive = false;
      thinkingCloseSeen = true;
      clearThinkingSpinner();
      scan = scan.substring(closeIdx + THINK_CLOSE_TAG.length);
    } else {
      showThinkingSpinnerTick();
    }
  }

  const keep = THINK_CLOSE_TAG.length + 16;
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
    while (visible.indexOf(THINK_OPEN_TAG) >= 0) {
      visible = visible.replace(THINK_OPEN_TAG, "");
    }
    while (visible.indexOf(THINK_CLOSE_TAG) >= 0) {
      visible = visible.replace(THINK_CLOSE_TAG, "");
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

function escapeJsonString(value: string): string {
  let out = "";
  for (let i = 0; i < value.length; i++) {
    const code = value.charCodeAt(i);
    if (code == 34) {
      out += '\\"';
    } else if (code == 92) {
      out += "\\\\";
    } else if (code == 10) {
      out += "\\n";
    } else if (code == 13) {
      out += "\\r";
    } else if (code == 9) {
      out += "\\t";
    } else {
      out += value.charAt(i);
    }
  }
  return out;
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

  const closeTag = "</think>";
  const closeIdx = raw.lastIndexOf(closeTag);
  if (closeIdx >= 0) {
    const after = raw.substring(closeIdx + closeTag.length).trim();
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
// Output truncation
// -----------------------------
function smartTruncate(text: string, maxChars: i32): string {
  if (text.length <= maxChars) return text;

  const lines = text.split("\n");
  if (lines.length < 20) {
    return "... (truncated)\n" + text.substring(text.length - maxChars);
  }

  const header = lines.slice(0, 5).join("\n");
  let remaining = maxChars - header.length - 50;
  if (remaining <= 0) return text.substring(text.length - maxChars);

  let tailStr = text.substring(text.length - remaining);
  const nextNl = tailStr.indexOf("\n");
  if (nextNl >= 0) tailStr = tailStr.substring(nextNl + 1);

  return header + "\n\n... (middle content truncated) ...\n\n" + tailStr;
}

// -----------------------------
// Runtime policy helpers
// -----------------------------
function shouldExit(inputText: string): bool {
  const v = inputText.trim().toLowerCase();
  return v == "exit" || v == "quit";
}

function isIncompleteCommand(cmd: string): bool {
  const c = cmd.trim();
  if (c.length == 0) return true;

  // Obvious broken shells
  if (c.endsWith("|") || c.endsWith("&&") || c.endsWith("||")) return true;

  // Your exact bad case
  if (c.indexOf("find ") == 0 && c.indexOf("-name") >= 0) {
    // Reject if "-name" is last token
    const parts = c.split(" ");
    if (parts.length > 0 && parts[parts.length - 1] == "-name") return true;
    // Reject if "-name" has empty pattern like: -name "" or -name ''
    if (c.indexOf('-name ""') >= 0 || c.indexOf("-name ''") >= 0) return true;
  }

  return false;
}

function looksUnsafe(cmd: string): bool {
  const c = cmd.trim().toLowerCase();

  // destructive-ish
  if (c.indexOf("rm ") >= 0) return true;
  if (c.indexOf("sudo") >= 0) return true;
  if (c.indexOf("dd ") >= 0) return true;
  if (c.indexOf("mkfs") >= 0) return true;

  // “find /” is expensive / permissiony on macOS; treat as unsafe/confirm
  if (c.indexOf("find /") >= 0) return true;

  return false;
}

// -----------------------------
// ReAct prompts
// -----------------------------
function buildInitialPrompt(user: string, osShort: string): string {
  return (
    "You are a CLI command planner operating in an iterative loop.\n" +
    "Final output must be a single JSON object. No markdown.\n" +
    "Schema:\n" +
    "{\n" +
    '  "kind": "command" | "answer",\n' +
    '  "command": "<single shell command or empty>",\n' +
    '  "confirm": true | false,\n' +
    '  "answer": "<final response or empty>"\n' +
    "}\n\n" +
    "Decision rule:\n" +
    '- If more info is needed, return kind="command" with ONE command.\n' +
    '- If info is sufficient, return kind="answer" with the final answer.\n' +
    "- After each command output, decide again.\n\n" +
    "Safety:\n" +
    "- Prefer read-only commands first.\n" +
    "- If a command is destructive/privileged/slow, set confirm=true.\n" +
    '- When kind="answer", command must be empty. When kind="command", answer must be empty.\n' +
    "\n" +
    "Platform note:\n" +
    "- Your commands are evaluated in a full `/bin/sh` shell environment. You CAN use pipes (`|`), boolean operators (`&&`), redirects, and subshells.\n" +
    "- If OS is macOS (Darwin): prefer `mdfind` / `/Applications` checks / `open -Ra` for app existence.\n" +
    "- Avoid `find /` unless absolutely necessary.\n\n" +
    "OS: " + osShort + "\n" +
    "New user request: " + user
  );
}

function buildFollowupPrompt(command: string, output: string): string {
  return (
    "Command executed:\n" + command + "\n\n" +
    "Command output:\n" + output + "\n\n" +
    "Decide next step now:\n" +
    '- If enough info, return kind="answer".\n' +
    '- Otherwise return next kind="command".\n' +
    "Final output must be a JSON object with schema (kind, command, confirm, answer)."
  );
}

function buildCancelPrompt(command: string): string {
  return (
    "User canceled this command:\n" + command + "\n\n" +
    "Return an alternative safer command, or return an answer if possible.\n" +
    "Final output must be a JSON object with schema (kind, command, confirm, answer)."
  );
}

function buildInvalidCommandPrompt(command: string): string {
  return (
    "Invalid or incomplete command was returned:\n" + command + "\n\n" +
    "Return ONE complete valid command or a final answer.\n" +
    "Final output must be a JSON object with schema (kind, command, confirm, answer)."
  );
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
