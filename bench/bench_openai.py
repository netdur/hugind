import asyncio
import time
import argparse
import os
from openai import AsyncOpenAI
import statistics

async def benchmark_request(client, model, prompt, request_id):
    start_time = time.time()
    ttft = None
    end_time = None
    token_count = 0

    try:
        stream = await client.chat.completions.create(
            model=model,
            messages=[{"role": "user", "content": prompt}],
            stream=True,
            max_tokens=100  # Limit output for consistent benchmarking
        )

        async for chunk in stream:
            if ttft is None:
                ttft = time.time() - start_time
            if chunk.choices and chunk.choices[0].delta.content:
                token_count += 1
        
        end_time = time.time()
    except Exception as e:
        print(f"Request {request_id} failed: {e}")
        return None

    total_time = end_time - start_time
    return {
        "request_id": request_id,
        "ttft": ttft,
        "total_time": total_time,
        "token_count": token_count,
        "tpot": (total_time - ttft) / (token_count - 1) if token_count > 1 else 0
    }

async def main():
    parser = argparse.ArgumentParser(description="Benchmark LLM endpoint")
    parser.add_argument("--base-url", type=str, default="http://localhost:8080/v1", help="Base URL for the API")
    parser.add_argument("--api-key", type=str, default="nopass", help="API Key")
    parser.add_argument("--model", type=str, default="gpt-3.5-turbo", help="Model name")
    parser.add_argument("--concurrency", type=int, default=10, help="Number of concurrent requests")
    parser.add_argument("--prompt", type=str, default="Explain quantum physics in one sentence.", help="Prompt to send")
    
    args = parser.parse_args()

    print(f"Benchmarking {args.base_url} with model {args.model}")
    print(f"Concurrency: {args.concurrency}")
    print(f"Prompt: {args.prompt}")
    print("-" * 50)

    client = AsyncOpenAI(
        base_url=args.base_url,
        api_key=args.api_key
    )

    tasks = []
    for i in range(args.concurrency):
        tasks.append(benchmark_request(client, args.model, args.prompt, i))

    results = await asyncio.gather(*tasks)
    results = [r for r in results if r is not None]

    if not results:
        print("No successful requests.")
        return

    ttfts = [r["ttft"] for r in results]
    total_times = [r["total_time"] for r in results]
    tpots = [r["tpot"] for r in results]

    print("\nResults:")
    print(f"Successful Requests: {len(results)}/{args.concurrency}")
    print(f"Avg TTFT: {statistics.mean(ttfts):.4f}s")
    print(f"Avg Total Time: {statistics.mean(total_times):.4f}s")
    print(f"Avg TPOT (Time Per Output Token): {statistics.mean(tpots):.4f}s")
    
    print("\nDetailed:")
    print(f"{'ID':<5} {'TTFT (s)':<10} {'Total (s)':<10} {'Tokens':<8} {'TPOT (s)':<10}")
    for r in results:
        print(f"{r['request_id']:<5} {r['ttft']:.4f:<10} {r['total_time']:.4f:<10} {r['token_count']:<8} {r['tpot']:.4f:<10}")

if __name__ == "__main__":
    asyncio.run(main())
