import { print, input, llmChat, runCommand, alloc } from "./wasm_sdk";
import { JSON } from "assemblyscript-json/assembly";

export { alloc };

class ParsedResponse {
  kind: string = "answer";
  command: string = "";
  confirm: bool = false;
  answer: string = "";
}

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

function llmJsonWithRetries(basePrompt: string, maxRetries: i32): string {
  let lastRaw = "";
  for (let i = 0; i < maxRetries; i++) {
    let prompt = basePrompt;
    if (i > 0) {
      prompt +=
        "\n\nERROR: Last output was not valid JSON. Raw Output:\n" +
        lastRaw +
        "\n\nReturn ONLY valid JSON object.";
    }
    const resp = llmChat(prompt);
    lastRaw = resp;
    print("LLM RAW: " + resp);

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

function scoreEvidence(cmd: string, output: string): i32 {
  const o = output.trim();
  if (o.length == 0) return 0;

  const low = o.toLowerCase();
  const c = cmd.trim().toLowerCase();

  // Empty result from PATH checks is weak evidence
  if ((c.startsWith("which ") || c.startsWith("command -v ") || c.startsWith("type ")) && o.length == 0) {
    return 0;
  }

  // Strong signals
  if (low.indexOf("android studio.app") >= 0) return 4;
  if (low.indexOf("/applications") >= 0) return 3;
  if (low.indexOf("version") >= 0) return 3;

  // Negative but real signals
  if (low.indexOf("no such file") >= 0) return 2;
  if (low.indexOf("not found") >= 0) return 2;
  if (low.indexOf("command not found") >= 0) return 1;

  return 1;
}

// -----------------------------
// Prompts (light “macOS optimization” only)
// -----------------------------
function buildInitialPrompt(user: string, osShort: string): string {
  return (
    "You are a CLI command planner. Reply ONLY as JSON. No markdown.\n" +
    "Schema:\n" +
    "{\n" +
    '  "kind": "command" | "answer",\n' +
    '  "command": "<single shell command or empty>",\n' +
    '  "confirm": true | false,\n' +
    '  "answer": "<final response or empty>"\n' +
    "}\n\n" +
    "Rules:\n" +
    "- Propose ONE command at a time when more info is needed.\n" +
    "- Prefer read-only commands.\n" +
    "- If the command could be destructive/privileged/slow, set confirm=true.\n" +
    '- When kind="answer", command must be empty. When kind="command", answer must be empty.\n' +
    "\n" +
    "Platform note:\n" +
    "- If OS is macOS (Darwin): prefer `mdfind` / `/Applications` checks / `open -Ra` for app existence.\n" +
    "- Avoid `find /` unless absolutely necessary (and then set confirm=true).\n\n" +
    "OS: " + osShort + "\n" +
    "User request: " + user
  );
}

function buildFollowupPrompt(user: string, command: string, output: string, osShort: string): string {
  return (
    "You are a CLI command planner. Reply ONLY as JSON. No markdown.\n" +
    "Schema:\n" +
    "{\n" +
    '  "kind": "command" | "answer",\n' +
    '  "command": "<single shell command or empty>",\n' +
    '  "confirm": true | false,\n' +
    '  "answer": "<final response or empty>"\n' +
    "}\n\n" +
    "OS: " + osShort + "\n" +
    "User request: " + user + "\n" +
    "Last command: " + command + "\n" +
    "Command output:\n" + output + "\n\n" +
    "If you need more info, propose the next command. Otherwise give the final answer."
  );
}

// -----------------------------
// Main
// -----------------------------
export function main(): void {
  print("CLIv1");

  const history: string[] = new Array<string>();

  // Probe OS
  const osShort = runCommand("uname -s").trim(); // "Darwin", "Linux", etc.
  const probeCmd = "uname -a";
  const probeOut = runCommand(probeCmd);
  history.push("CMD: " + probeCmd + "\nOUT:\n" + smartTruncate(probeOut, 2000));

  while (true) {
    const user = input("> ").trim();
    if (user.length == 0) continue;
    if (shouldExit(user)) break;

    // Per-request: require at least 10 executed commands before accepting “give up”
    const maxAttempts: i32 = 10;
    const evidenceThreshold: i32 = 3;

    let attempts: i32 = 0;
    let evidence: i32 = 0;

    let prompt = buildInitialPrompt(user, osShort) + "\n\nHistory:\n" + history.join("\n\n");
    let response = llmJsonWithRetries(prompt, 10);
    let parsed = parseResponse(response);

    for (let step = 0; step < 50; step++) {
      if (parsed.kind == "answer") {
        const haveEnoughEvidence = evidence >= evidenceThreshold;
        const outOfAttempts = attempts >= maxAttempts;

        if (!haveEnoughEvidence && !outOfAttempts) {
          // Runtime policy: keep trying; don’t accept early finalization
          const force =
            "Return JSON with kind=command. Do not finalize yet.\n\n" +
            "User request: " + user + "\n\nHistory:\n" + history.join("\n\n") +
            "\n\nAttempts so far: " + attempts.toString() + "/" + maxAttempts.toString();
          response = llmJsonWithRetries(force, 10);
          parsed = parseResponse(response);
          continue;
        }

        // Accept answer: either enough evidence, or we truly tried 10 times
        print(parsed.answer);
        break;
      }

      if (parsed.kind == "command") {
        let cmd = parsed.command.trim();

        if (isIncompleteCommand(cmd)) {
          const msg =
            "Your command was incomplete/invalid. Return JSON with a complete single command (kind=command).";
          response = llmJsonWithRetries(msg + "\n\nUser request: " + user, 10);
          parsed = parseResponse(response);
          continue;
        }

        // Enforce confirm at runtime for unsafe/slow commands, regardless of LLM flag
        let needConfirm = parsed.confirm || looksUnsafe(cmd);

        if (needConfirm) {
          const decision = input("Run command? " + cmd + " (y/n) ").trim().toLowerCase();
          if (!(decision == "y" || decision == "yes")) {
            // User canceled: still counts as a “try”, and we continue (don’t end task)
            attempts++;
            history.push("CMD: " + cmd + "\nOUT:\nUser canceled execution.");
            if (history.length > 6) history.shift();

            const followCancel =
              buildFollowupPrompt(user, cmd, "User canceled execution.", osShort) +
              "\n\nHistory:\n" + history.join("\n\n");
            response = llmJsonWithRetries(followCancel, 10);
            parsed = parseResponse(response);
            continue;
          }
        }

        // Execute
        const rawOut = runCommand(cmd);
        attempts++;

        // Evidence update
        evidence += scoreEvidence(cmd, rawOut);

        // Keep prompts bounded
        const outForPrompt = smartTruncate(rawOut, 4000);
        const outForHistory = smartTruncate(rawOut, 2000);

        history.push("CMD: " + cmd + "\nOUT:\n" + outForHistory);
        if (history.length > 6) history.shift();

        const follow =
          buildFollowupPrompt(user, cmd, outForPrompt, osShort) +
          "\n\nProgress:\n" +
          "- Attempts: " + attempts.toString() + "/" + maxAttempts.toString() + "\n" +
          "- Evidence: " + evidence.toString() + " (threshold " + evidenceThreshold.toString() + ")\n\n" +
          "History:\n" + history.join("\n\n");

        response = llmJsonWithRetries(follow, 10);
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
  }
}
