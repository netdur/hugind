#!/usr/bin/env python3

import json
import sys
import urllib.error
import urllib.request

# Configure everything here.
SERVER_URL = "http://localhost:8080/v1/chat/completions"
MODEL = "Qwen3.5-9B-GGUF"
PROMPT = "Write a short poem about coding"
ENABLE_THINKING = True  # True | False
# THINKING_BUDGET = None  # None = no budget override, or set int like 512
THINKING_BUDGET = 256
RESPONSE_FORMAT_JSON = False  # True => {"response_format": {"type": "json_object"}}
MAX_TOKENS = 16_000
REQUEST_TIMEOUT_SECONDS = 60000


def build_payload() -> dict:
    payload = {
        "model": MODEL,
        "messages": [{"role": "user", "content": PROMPT}],
        "stream": True,
        "max_tokens": MAX_TOKENS,
        "enable_thinking": ENABLE_THINKING,
    }
    if RESPONSE_FORMAT_JSON:
        payload["response_format"] = {"type": "json_object"}
    if THINKING_BUDGET is not None:
        payload["thinking_budget_tokens"] = THINKING_BUDGET
    return payload


def unescape_display_text(text: str) -> str:
    # Convert literal escaped sequences to terminal formatting.
    return text.replace("\\r\\n", "\n").replace("\\n", "\n").replace("\\t", "\t")


def extract_text_from_payload(obj: dict) -> str:
    choices = obj.get("choices")
    if not isinstance(choices, list) or not choices:
        return ""
    first = choices[0] if isinstance(choices[0], dict) else {}
    delta = first.get("delta")
    if isinstance(delta, dict):
        content = delta.get("content")
        if isinstance(content, str):
            return content
    message = first.get("message")
    if isinstance(message, dict):
        content = message.get("content")
        if isinstance(content, str):
            return content
    return ""


def emit_new_think_close_markers(cumulative: str, previous_count: int) -> int:
    count = cumulative.count("</think>")
    if count > previous_count:
        for idx in range(previous_count + 1, count + 1):
            print(f"\n\n[thinking closed #{idx}]\n", file=sys.stderr, flush=True)
    return count


def process_json_payload(
    payload_str: str,
    cumulative: str,
    printed_any_text: bool,
    think_close_count: int,
) -> tuple[str, bool, int]:
    try:
        obj = json.loads(payload_str)
    except json.JSONDecodeError:
        return cumulative, printed_any_text, think_close_count

    err = obj.get("error")
    if isinstance(err, dict):
        msg = err.get("message")
        if isinstance(msg, str) and msg:
            print(f"\n[error] {msg}", file=sys.stderr, flush=True)

    text = extract_text_from_payload(obj)
    if text:
        printed_any_text = True
        cumulative += text
        sys.stdout.write(unescape_display_text(text))
        sys.stdout.flush()
        think_close_count = emit_new_think_close_markers(cumulative, think_close_count)

    return cumulative, printed_any_text, think_close_count


def stream_response(resp) -> None:
    printed_any_text = False
    saw_stream_data = False
    cumulative = ""
    think_close_count = 0
    non_stream_lines: list[str] = []

    for raw in resp:
        line = raw.decode("utf-8", errors="replace").rstrip("\r\n")
        if not line:
            continue

        if line.startswith("data:"):
            saw_stream_data = True
            data = line[5:].lstrip()
            if data == "[DONE]":
                break
            cumulative, printed_any_text, think_close_count = process_json_payload(
                data, cumulative, printed_any_text, think_close_count
            )
            continue

        non_stream_lines.append(line)

    if saw_stream_data:
        sys.stdout.write("\n")
        sys.stdout.flush()
        if not printed_any_text:
            print(
                "[warn] Stream received but no printable text chunks were found.",
                file=sys.stderr,
            )
        return

    # Fallback: plain JSON response even though stream=true was requested.
    body = "".join(non_stream_lines).strip()
    if not body:
        print(
            "[warn] No SSE data received. Check server URL/port and that stream=true responses are enabled.",
            file=sys.stderr,
        )
        return

    cumulative, printed_any_text, think_close_count = process_json_payload(
        body, cumulative, printed_any_text, think_close_count
    )
    sys.stdout.write("\n")
    sys.stdout.flush()
    if not printed_any_text:
        print("[warn] Response contained no printable content.", file=sys.stderr)


def main() -> int:
    if not isinstance(ENABLE_THINKING, bool):
        print(
            f"Invalid ENABLE_THINKING value: {ENABLE_THINKING!r} (expected bool)",
            file=sys.stderr,
        )
        return 2
    if THINKING_BUDGET is not None and (
        not isinstance(THINKING_BUDGET, int) or THINKING_BUDGET < 0
    ):
        print(
            f"Invalid THINKING_BUDGET value: {THINKING_BUDGET!r} (expected non-negative int or None)",
            file=sys.stderr,
        )
        return 2

    payload = build_payload()
    body = json.dumps(payload).encode("utf-8")

    print("Testing Chat Completion (Streaming Plain Text)", flush=True)
    print(f"Target:         {SERVER_URL}", flush=True)
    print(f"Model:          {MODEL}", flush=True)
    print(f"Max tokens:     {MAX_TOKENS}", flush=True)
    print(f"Thinking:       {str(ENABLE_THINKING).lower()}", flush=True)
    print(
        f"Thinking budget:{THINKING_BUDGET if THINKING_BUDGET is not None else '<none>'}",
        flush=True,
    )
    print(
        f"Response format:{'json_object' if RESPONSE_FORMAT_JSON else '<none>'}",
        flush=True,
    )
    print(f"Prompt:         {PROMPT}", flush=True)
    print("-------------------------------------", flush=True)

    req = urllib.request.Request(
        SERVER_URL,
        data=body,
        headers={"Content-Type": "application/json"},
        method="POST",
    )

    try:
        with urllib.request.urlopen(req, timeout=REQUEST_TIMEOUT_SECONDS) as resp:
            stream_response(resp)
    except urllib.error.HTTPError as exc:
        details = ""
        try:
            details = exc.read().decode("utf-8", errors="replace").strip()
        except Exception:
            details = ""
        print(f"[error] HTTP {exc.code}: {exc.reason}", file=sys.stderr)
        if details:
            print(details, file=sys.stderr)
        return 1
    except urllib.error.URLError as exc:
        print(f"[error] Connection failed: {exc}", file=sys.stderr)
        return 1
    except Exception as exc:
        print(f"[error] Unexpected failure: {exc}", file=sys.stderr)
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
