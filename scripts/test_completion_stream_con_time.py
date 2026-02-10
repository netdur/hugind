#!/usr/bin/env python3

import json
import threading
import time
import urllib.request

SERVER_URL = "http://localhost:8080/v1/chat/completions"
MODEL = "gemma-3-4b-it"

PROMPTS = [
    "summarize the plot of Romeo and Juliet",
    "who are you?",
    "tell me a joke",
    "who are you?",
    "tell me a story about dogs",
    "who are you?",
    "tell me a story about cats",
    "who are you?",
]


def stream_request(prompt, timings, idx, print_lock):
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
    start = time.time()
    first_byte = None
    done = None
    try:
        with urllib.request.urlopen(req, timeout=600) as resp:
            while True:
                line = resp.readline()
                if not line:
                    break
                if first_byte is None:
                    first_byte = time.time()
                try:
                    text = line.decode("utf-8", errors="ignore").strip()
                except Exception:
                    continue
                if not text.startswith("data: "):
                    continue
                data = text[6:].strip()
                if data == "[DONE]":
                    done = time.time()
                    break
    except Exception:
        done = time.time()
    if done is None:
        done = time.time()
    if first_byte is None:
        first_byte = done
    timings[idx] = (start, first_byte, done)
    with print_lock:
        t_request = first_byte - start
        t_done = done - start
        print(f"#{idx + 1} time-to-request={t_request:.3f}s time-to-done={t_done:.3f}s")


def main():
    timings = [None] * len(PROMPTS)
    print_lock = threading.Lock()
    threads = []
    for i, prompt in enumerate(PROMPTS):
        t = threading.Thread(
            target=stream_request,
            args=(prompt, timings, i, print_lock),
            daemon=True,
        )
        threads.append(t)
        t.start()

    for t in threads:
        t.join()

    # All requests completed.


if __name__ == "__main__":
    main()
