import json
import time
import argparse
import http.client
import threading
import statistics
import random
from urllib.parse import urlparse

# Configuration for Retries
MAX_RETRIES = 5
BACKOFF_FACTOR = 2  # Seconds to wait between retries (multiplied by attempt number)

def benchmark_request(base_url, model, prompt, request_id, results_list):
    parsed_url = urlparse(base_url)
    host = parsed_url.hostname
    port = parsed_url.port or (443 if parsed_url.scheme == 'https' else 80)
    path = parsed_url.path.rstrip('/') + "/chat/completions"

    payload = {
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "stream": True,
        "max_tokens": 100
    }
    
    headers = {
        "Content-Type": "application/json",
        "Authorization": "Bearer nopass",
    }

    attempt = 0
    while attempt < MAX_RETRIES:
        start_time = time.time()
        ttft = None
        token_count = 0
        
        try:
            conn = http.client.HTTPConnection(host, port, timeout=30)
            conn.request("POST", path, body=json.dumps(payload), headers=headers)
            response = conn.getresponse()
            
            # Check if server rejected request (e.g., 429 Too Many Requests or 503)
            if response.status != 200:
                raise Exception(f"HTTP {response.status}")

            while True:
                line = response.fp.readline()
                if not line:
                    break
                    
                line = line.decode('utf-8').strip()
                if not line or not line.startswith("data: "):
                    continue
                    
                data_str = line[6:]
                if data_str == "[DONE]":
                    break
                    
                try:
                    data = json.loads(data_str)
                    if not ttft:
                        ttft = time.time() - start_time
                    
                    if "choices" in data and len(data["choices"]) > 0:
                        delta = data["choices"][0].get("delta", {})
                        if "content" in delta:
                            token_count += 1
                except json.JSONDecodeError:
                    continue

            end_time = time.time()
            conn.close()

            total_time = end_time - start_time
            tpot = (total_time - ttft) / (token_count - 1) if token_count > 1 else 0

            results_list.append({
                "request_id": request_id,
                "ttft": ttft,
                "total_time": total_time,
                "token_count": token_count,
                "tpot": tpot
            })
            
            # REQUIREMENT: Print when finished
            print(f"req {request_id} finished in {total_time:.2f} seconds")
            return # Success! Exit the retry loop

        except Exception as e:
            attempt += 1
            wait_time = (BACKOFF_FACTOR * attempt) + random.uniform(0, 1)
            if attempt < MAX_RETRIES:
                # print(f"req {request_id} failed ({e}), retrying in {wait_time:.1f}s...")
                time.sleep(wait_time)
            else:
                print(f"req {request_id} failed permanently after {MAX_RETRIES} attempts.")

def main():
    parser = argparse.ArgumentParser(description="LLM Benchmark with Retries")
    parser.add_argument("--base-url", type=str, default="http://localhost:8080/v1")
    parser.add_argument("--model", type=str, default="gpt-3.5-turbo")
    parser.add_argument("--total", type=int, default=100, help="Total requests to send")
    parser.add_argument("--concurrency", type=int, default=25, help="Max parallel requests")
    parser.add_argument("--prompt", type=str, default="Explain quantum physics in one sentence.")
    
    args = parser.parse_args()

    print(f"Sending {args.total} total requests (Concurrency: {args.concurrency})")
    print("-" * 50)

    threads = []
    results = []
    
    # Use a Semaphore to limit active threads to your hardware capacity (25)
    # but still loop until we reach the total (100)
    semaphore = threading.Semaphore(args.concurrency)

    def worker(req_id):
        with semaphore:
            benchmark_request(args.base_url, args.model, args.prompt, req_id, results)

    for i in range(args.total):
        t = threading.Thread(target=worker, args=(i,))
        threads.append(t)
        t.start()

    for t in threads:
        t.join()

    # Final Statistics
    if not results:
        return

    total_times = [r["total_time"] for r in results]
    print("-" * 50)
    print(f"Completed {len(results)}/{args.total} requests successfully.")
    print(f"Average response time: {statistics.mean(total_times):.2f}s")

if __name__ == "__main__":
    main()