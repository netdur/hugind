#!/usr/bin/env python3

import json
import threading
import time
import urllib.request

from rich.console import Console, Group
from rich.live import Live
from rich.panel import Panel
from rich.text import Text

SERVER_URL = "http://localhost:8080/v1/chat/completions"
MODEL = "gemma-3-4b-it"

PROMPTS = [
    "Give me a one-sentence summary of the moon.",
    "Write a haiku about wind.",
    "List three uses of a paperclip.",
    "Explain photosynthesis in one sentence.",
    "Name two benefits of exercise.",
    "Give one tip for saving time.",
    "Write a rhyming couplet about rain.",
    "List three colors you like.",
    "Say hello in a creative way.",
]


def stream_request(prompt, out_buf, done_flag):
    payload = {
        "model": MODEL,
        "messages": [{"role": "user", "content": prompt}],
        "stream": True,
    }
    data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        SERVER_URL,
        data=data,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=600) as resp:
            while True:
                line = resp.readline()
                if not line:
                    break
                try:
                    text = line.decode("utf-8", errors="ignore").strip()
                except Exception:
                    continue
                if not text.startswith("data: "):
                    continue
                data = text[6:].strip()
                if data == "[DONE]":
                    break
                try:
                    obj = json.loads(data)
                    delta = obj["choices"][0]["delta"]
                    content = delta.get("content")
                    if content:
                        out_buf.append(content)
                except Exception:
                    continue
    except Exception as exc:
        out_buf.append(f"\n[error] {exc}\n")
    finally:
        done_flag.set()


def render_panels(buffers):
    panels = []
    for i, buf in enumerate(buffers, 1):
        text = "".join(buf) if buf else ""
        panels.append(Panel(Text(text), title=f"#{i} results", border_style="cyan"))
    return Group(*panels)


def main():
    console = Console()
    buffers = [[] for _ in PROMPTS]
    done_flags = [threading.Event() for _ in PROMPTS]

    threads = []
    for i in range(len(PROMPTS)):
        t = threading.Thread(
            target=stream_request,
            args=(PROMPTS[i], buffers[i], done_flags[i]),
            daemon=True,
        )
        threads.append(t)
        t.start()

    console.print("Sending 3 streaming requests...\n")
    with Live(refresh_per_second=8, console=console) as live:
        while not all(f.is_set() for f in done_flags):
            live.update(render_panels(buffers))
            time.sleep(0.1)
        live.update(render_panels(buffers))

    for t in threads:
        t.join(timeout=0.1)


if __name__ == "__main__":
    main()
