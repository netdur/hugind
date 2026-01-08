import json
import time
import argparse
import http.client
import threading
import statistics
from urllib.parse import urlparse

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
        "Authorization": "Bearer nopass"
    }

    start_time = time.time()
    ttft = None
    token_count = 0
    
    try:
        conn = http.client.HTTPConnection(host, port)
        conn.request("POST", path, body=json.dumps(payload), headers=headers)
        response = conn.getresponse()
        
        while True:
            line = response.fp.readline()
            if not line:
                break
                
            line = line.decode('utf-8').strip()
            if not line:
                continue
                
            if line.startswith("data: "):
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
        tpot = 0
        if token_count > 1:
            tpot = (total_time - ttft) / (token_count - 1)

        results_list.append({
            "request_id": request_id,
            "ttft": ttft,
            "total_time": total_time,
            "token_count": token_count,
            "tpot": tpot
        })

    except Exception as e:
        print(f"Request {request_id} failed: {e}")

def main():
    parser = argparse.ArgumentParser(description="Benchmark Hugind Server")
    parser.add_argument("--base-url", type=str, default="http://localhost:8080/v1", help="Hugind API URL")
    parser.add_argument("--model", type=str, default="gpt-3.5-turbo", help="Model name")
    parser.add_argument("--concurrency", type=int, default=2, help="Number of concurrent requests")
    parser.add_argument("--prompt", type=str, default="Explain quantum physics in one sentence.", help="Prompt to send")
    
    args = parser.parse_args()

    print(f"Benchmarking Hugind at {args.base_url}")
    print(f"Model: {args.model}")
    print(f"Concurrency: {args.concurrency}")
    print(f"Prompt: {args.prompt}")
    print("-" * 50)

    threads = []
    results = []

    for i in range(args.concurrency):
        t = threading.Thread(target=benchmark_request, args=(args.base_url, args.model, args.prompt, i, results))
        threads.append(t)
        t.start()

    for t in threads:
        t.join()

    if not results:
        print("No successful requests.")
        return

    ttfts = [r["ttft"] for r in results if r["ttft"] is not None]
    total_times = [r["total_time"] for r in results]
    tpots = [r["tpot"] for r in results]

    if not ttfts:
        print("No tokens received.")
        return

    print("\nResults:")
    print(f"Successful Requests: {len(results)}/{args.concurrency}")
    print(f"Avg TTFT: {statistics.mean(ttfts):.4f}s")
    print(f"Avg Total Time: {statistics.mean(total_times):.4f}s")
    print(f"Avg TPOT: {statistics.mean(tpots):.4f}s")
    
    print("\nDetailed:")
    print(f"{'ID':<5} {'TTFT (s)':<10} {'Total (s)':<10} {'Tokens':<8} {'TPOT (s)':<10}")
    # Sort by ID for cleaner output
    results.sort(key=lambda x: x["request_id"])
    for r in results:
        t_ttft = f"{r['ttft']:.4f}" if r['ttft'] else "N/A"
        t_total = f"{r['total_time']:.4f}"
        t_tpot = f"{r['tpot']:.4f}"
        print(f"{r['request_id']:<5} {t_ttft:<10} {t_total:<10} {r['token_count']:<8} {t_tpot:<10}")

if __name__ == "__main__":
    main()
