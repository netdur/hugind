// @ts-nocheck

export function escapeJsonString(value: string): string {
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

export function buildInitialPrompt(user: string, osShort: string): string {
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

export function buildFollowupPrompt(command: string, output: string): string {
  return (
    "Command executed:\n" + command + "\n\n" +
    "Command output:\n" + output + "\n\n" +
    "Decide next step now:\n" +
    '- If enough info, return kind="answer".\n' +
    '- Otherwise return next kind="command".\n' +
    "Final output must be a JSON object with schema (kind, command, confirm, answer)."
  );
}

export function buildCancelPrompt(command: string): string {
  return (
    "User canceled this command:\n" + command + "\n\n" +
    "Return an alternative safer command, or return an answer if possible.\n" +
    "Final output must be a JSON object with schema (kind, command, confirm, answer)."
  );
}

export function buildInvalidCommandPrompt(command: string): string {
  return (
    "Invalid or incomplete command was returned:\n" + command + "\n\n" +
    "Return ONE complete valid command or a final answer.\n" +
    "Final output must be a JSON object with schema (kind, command, confirm, answer)."
  );
}
