#!/usr/bin/env python3

import json
import os
import sys
import urllib.request
import subprocess


SERVER_URL = os.getenv("HUGIND_SERVER_URL", "http://localhost:8084/v1/chat/completions")
MODEL = os.getenv("HUGIND_MODEL", "AgentCPM-Explore")
TEMPERATURE = float(os.getenv("HUGIND_TEMPERATURE", "0.8"))
TOP_P = float(os.getenv("HUGIND_TOP_P", "0.9"))
MAX_TOKENS = int(os.getenv("HUGIND_MAX_TOKENS", "1024"))
SESSION_ID = os.getenv("HUGIND_SESSION_ID")
SYSTEM_PROMPT = os.getenv(
    "HUGIND_SYSTEM_PROMPT",
    "You are an agent. Respond ONLY with valid JSON using this schema: "
    '{"thinking":"...","command":"..."} . '
    "The command field should be a shell command to run, or an empty string.",
)


def stream_chat(messages):
    payload = {
        "model": MODEL,
        "messages": messages,
        "stream": True,
        "temperature": TEMPERATURE,
        "top_p": TOP_P,
        "max_tokens": MAX_TOKENS,
    }
    data = json.dumps(payload).encode("utf-8")
    headers = {"Content-Type": "application/json"}
    if SESSION_ID:
        headers["X-Session-Id"] = SESSION_ID

    req = urllib.request.Request(
        SERVER_URL,
        data=data,
        headers=headers,
        method="POST",
    )

    with urllib.request.urlopen(req, timeout=600) as resp:
        while True:
            line = resp.readline()
            if not line:
                break
            text = line.decode("utf-8", errors="ignore").strip()
            if not text.startswith("data: "):
                continue
            data = text[6:].strip()
            if data == "[DONE]":
                break
            try:
                obj = json.loads(data)
            except json.JSONDecodeError:
                continue
            delta = obj.get("choices", [{}])[0].get("delta", {})
            content = delta.get("content")
            if content:
                yield content


def extract_json(text):
    candidates = []
    depth = 0
    start = None
    for i, ch in enumerate(text):
        if ch == "{":
            if depth == 0:
                start = i
            depth += 1
        elif ch == "}":
            if depth > 0:
                depth -= 1
                if depth == 0 and start is not None:
                    candidates.append(text[start : i + 1])
                    start = None
    for snippet in reversed(candidates):
        try:
            return json.loads(snippet)
        except json.JSONDecodeError:
            continue
    return None


def main():
    print("Agent CLI (streaming)")
    print(f"Server: {SERVER_URL}")
    print(f"Model:  {MODEL}")
    print("Type 'exit' or 'quit' to stop.\n")

    messages = [{"role": "system", "content": SYSTEM_PROMPT}]
    while True:
        try:
            user_input = input("You> ").strip()
        except (EOFError, KeyboardInterrupt):
            print()
            break
        if not user_input:
            continue
        if user_input.lower() in {"exit", "quit"}:
            break

        messages.append({"role": "user", "content": user_input})
        print("Assistant> ", end="", flush=True)
        chunks = []
        try:
            for chunk in stream_chat(messages):
                chunks.append(chunk)
                sys.stdout.write(chunk)
                sys.stdout.flush()
        except Exception as exc:
            print(f"\n[error] {exc}")
            continue

        full_response = "".join(chunks).strip()
        print()
        cmd = ""
        result_text = ""
        obj = extract_json(full_response)
        if obj is not None:
            cmd = str(obj.get("command", "") or "").strip()

        if cmd:
            try:
                answer = input(f"Run command? `{cmd}` [y/N] ").strip().lower()
            except (EOFError, KeyboardInterrupt):
                print()
                break
            if answer in {"y", "yes"}:
                try:
                    result = subprocess.run(
                        cmd,
                        shell=True,
                        check=False,
                        capture_output=True,
                        text=True,
                    )
                    if result.stdout:
                        print(result.stdout, end="" if result.stdout.endswith("\n") else "\n")
                        result_text += result.stdout
                    if result.stderr:
                        print(result.stderr, end="" if result.stderr.endswith("\n") else "\n")
                        result_text += result.stderr
                except Exception as exc:
                    print(f"[error] failed to run command: {exc}")
                    result_text = f"[error] failed to run command: {exc}"

        history_payload = {"command": cmd, "result": result_text}
        messages.append({"role": "assistant", "content": json.dumps(history_payload)})


if __name__ == "__main__":
    main()
