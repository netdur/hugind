// @ts-nocheck
import { extractJson, parseResponse } from "./lib/json";
import { isIncompleteCommand, looksUnsafe, shouldExit } from "./lib/safety";
import { smartTruncate } from "./lib/truncate";
import { buildInitialPrompt, buildInvalidCommandPrompt, escapeJsonString } from "./lib/prompt";

function ok(value: bool): i32 {
  return value ? 1 : 0;
}

export function unit_extract_json_direct(): i32 {
  const input = '{"kind":"answer","answer":"ok"}';
  return ok(extractJson(input) == input);
}

export function unit_extract_json_fenced(): i32 {
  const input = "before\n```json\n{\"kind\":\"command\",\"command\":\"ls\"}\n```\nafter";
  return ok(extractJson(input) == '{"kind":"command","command":"ls"}');
}

export function unit_parse_response_normalizes_command_shape(): i32 {
  const parsed = parseResponse('{"kind":"answer","command":"ls","confirm":true,"answer":"ignored"}');
  return ok(parsed.kind == "command" && parsed.command == "ls" && parsed.confirm == true);
}

export function unit_parse_response_fallback_answer(): i32 {
  const parsed = parseResponse("not json");
  return ok(parsed.kind == "answer" && parsed.answer == "not json");
}

export function unit_safety_classification(): i32 {
  return ok(
    looksUnsafe("rm -rf /tmp/x") &&
      !looksUnsafe("ls -la") &&
      isIncompleteCommand("echo hi |") &&
      isIncompleteCommand("find . -name") &&
      !isIncompleteCommand('find . -name "*.ts"')
  );
}

export function unit_should_exit_detection(): i32 {
  return ok(shouldExit("exit") && shouldExit(" Quit ") && !shouldExit("continue"));
}

export function unit_smart_truncate_marker(): i32 {
  let text = "";
  for (let i = 0; i < 40; i++) {
    text += "line" + i.toString() + "\n";
  }
  const out = smartTruncate(text, 120);
  return ok(out.indexOf("middle content truncated") >= 0);
}

export function unit_escape_json_string_behaviour(): i32 {
  const out = escapeJsonString('a\"b\\c\nd\re\tf');
  return ok(out == 'a\\\"b\\\\c\\nd\\re\\tf');
}

export function unit_initial_prompt_contains_context(): i32 {
  const prompt = buildInitialPrompt("list files", "Darwin");
  return ok(
    prompt.indexOf("OS: Darwin") >= 0 &&
      prompt.indexOf("New user request: list files") >= 0 &&
      prompt.indexOf("Final output must be a single JSON object") >= 0
  );
}

export function unit_invalid_command_prompt_contains_command(): i32 {
  const prompt = buildInvalidCommandPrompt("find . -name");
  return ok(prompt.indexOf("find . -name") >= 0);
}
