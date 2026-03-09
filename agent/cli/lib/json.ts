// @ts-nocheck
import { JSON } from "assemblyscript-json/assembly";

export class ParsedResponse {
  kind: string = "answer";
  command: string = "";
  confirm: bool = false;
  answer: string = "";
}

export function extractJson(text: string): string {
  const trimmed = text.trim();
  if (trimmed.startsWith("{") && trimmed.endsWith("}")) return trimmed;

  const startMarker = "```json";
  const endMarker = "```";
  const start = trimmed.indexOf(startMarker);
  if (start >= 0) {
    const after = start + startMarker.length;
    const end = trimmed.indexOf(endMarker, after);
    if (end > after) return trimmed.substring(after, end).trim();
  }

  const a = trimmed.indexOf("{");
  const b = trimmed.lastIndexOf("}");
  if (a >= 0 && b > a) return trimmed.substring(a, b + 1);

  return "";
}

export function parseJsonResponse(text: string): ParsedResponse | null {
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

  if (kind == "command" || command.length > 0) {
    return { kind: "command", command, confirm, answer: "" };
  }
  return { kind: "answer", command: "", confirm: false, answer };
}

export function parseResponse(text: string): ParsedResponse {
  const jsonParsed = parseJsonResponse(text);
  if (jsonParsed != null) return jsonParsed as ParsedResponse;

  return { kind: "answer", command: "", confirm: false, answer: text.trim() };
}
