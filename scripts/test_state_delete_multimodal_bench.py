#!/usr/bin/env python3

import base64
import json
import os
import time
import urllib.request
from urllib.error import HTTPError, URLError


SERVER_URL = os.getenv("SERVER_URL", "http://localhost:8081/v1/chat/completions")
STATE_DELETE_URL = os.getenv("STATE_DELETE_URL", "http://localhost:8081/v1/state")
STATE_SAVE_URL = os.getenv("STATE_SAVE_URL", "http://localhost:8081/v1/state/save")
STATE_IDLE_URL = os.getenv("STATE_IDLE_URL", "http://localhost:8081/v1/state/idle")
MODEL = os.getenv("MODEL", "gemma-3-4b-it")
SESSION_ID = os.getenv("SESSION_ID", "session_madonna_test")
TEMPLATE_ID = os.getenv("TEMPLATE_ID", "session_madonna_test")
IMAGE_PATH = os.getenv("IMAGE_PATH", "assets/madonna.jpg")
CACHE_FILE = os.getenv("CACHE_FILE", f"cache/{SESSION_ID}.bin")


def now_ms() -> int:
    return int(time.time() * 1000)


def request_json(method: str, url: str, payload=None, headers=None):
    data = None
    if payload is not None:
        data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Content-Type", "application/json")
    if headers:
        for k, v in headers.items():
            req.add_header(k, v)
    try:
        with urllib.request.urlopen(req) as resp:
            body = resp.read().decode("utf-8")
            return resp.status, body
    except HTTPError as e:
        body = e.read().decode("utf-8") if e.fp else str(e)
        return e.code, body
    except URLError as e:
        return 0, str(e)


def print_response(body: str):
    try:
        data = json.loads(body)
        content = data.get("choices", [{}])[0].get("message", {}).get("content")
        if content is None:
            print(body)
        else:
            print(content)
    except json.JSONDecodeError:
        print("Non-JSON response:")
        print(body)


def main():
    with open(IMAGE_PATH, "rb") as f:
        image_b64 = base64.b64encode(f.read()).decode("ascii")
    image_url = f"data:image/jpeg;base64,{image_b64}"

    print("Step 1: image question (environment) with request id")
    payload = {
        "model": MODEL,
        "messages": [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "What is the environment (forest, beach, etc)?"},
                    {"type": "image_url", "image_url": {"url": image_url}},
                ],
            }
        ],
        "stream": False,
    }
    start = time.perf_counter()
    _, body = request_json(
        "POST", SERVER_URL, payload, headers={"X-Request-ID": SESSION_ID}
    )
    end = time.perf_counter()
    print_response(body)
    print(f"Time (ms): {int((end - start) * 1000)}")
    print()

    print("Step 2: text-only question (hair color)")
    payload = {
        "model": MODEL,
        "messages": [{"role": "user", "content": "What is the color of her hair?"}],
        "stream": False,
    }
    start = time.perf_counter()
    _, body = request_json(
        "POST", SERVER_URL, payload, headers={"X-Request-ID": SESSION_ID}
    )
    end = time.perf_counter()
    print_response(body)
    print(f"Time (ms): {int((end - start) * 1000)}")
    print()

    print("Step 3: evict session to disk (vram -> ram -> disk)")
    request_json("POST", STATE_IDLE_URL, {"session_id": SESSION_ID})
    time.sleep(0.2)
    request_json(
        "POST", STATE_SAVE_URL, {"session_id": SESSION_ID, "template_id": TEMPLATE_ID}
    )
    time.sleep(0.2)

    print("Step 4: text-only question (hair color) after disk eviction")
    payload = {
        "model": MODEL,
        "messages": [{"role": "user", "content": "What is the color of her hair?"}],
        "stream": False,
    }
    start = time.perf_counter()
    _, body = request_json(
        "POST", SERVER_URL, payload, headers={"X-Request-ID": SESSION_ID}
    )
    end = time.perf_counter()
    print_response(body)
    print(f"Time (ms): {int((end - start) * 1000)}")
    print()

    print("Step 5: delete session state (free vram/ram + delete file)")
    request_json("DELETE", f"{STATE_DELETE_URL}/{SESSION_ID}")
    print()

    print("Step 6: text-only question (hair color) after delete (should not know)")
    payload = {
        "model": MODEL,
        "messages": [{"role": "user", "content": "What is the color of her hair?"}],
        "stream": False,
    }
    start = time.perf_counter()
    _, body = request_json(
        "POST", SERVER_URL, payload, headers={"X-Request-ID": SESSION_ID}
    )
    end = time.perf_counter()
    print_response(body)
    print(f"Time (ms): {int((end - start) * 1000)}")
    print()

    print("Step 7: verify cache file removed")
    if os.path.isfile(CACHE_FILE):
        print(f"File still present: {CACHE_FILE}")
    else:
        print(f"File removed: {CACHE_FILE}")


if __name__ == "__main__":
    main()
