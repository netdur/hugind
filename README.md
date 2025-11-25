# Hugind

Hugind is a native, stateful inference server for local LLMs. It wraps `llama.cpp` via `llama_cpp_dart`, manages your GGUF library, generates hardware-aware configs, and serves an OpenAI-compatible API with automatic session persistence across VRAM, RAM, and disk.

## Features
- Native performance with Metal, CUDA, or CPU-only presets (Flash Attention, mmap/offload tuned per platform)
- Stateful slots: per-user KV cache that hibernates to RAM/disk when VRAM is full, then resumes instantly
- OpenAI-compatible `/v1/chat/completions` endpoint plus `/v1/models` and `/health`
- Multi-tenant by design: configurable workers (`concurrency`) and per-worker slot limits (`max_slots`)
- Guided setup: hardware probe, context-size calculator, chat-format templates, and interactive model download

## Installation
```bash
brew install hugind
```

Or build locally (requires Dart):
```bash
git clone https://github.com/your-username/hugind.git
cd hugind
bash build.sh
export PATH="$PATH:$(pwd)/bin"
```

One-time defaults (library path and optional Hugging Face token):
```bash
hugind config defaults --lib /path/to/libllama.dylib   # macOS
# or
hugind config defaults --lib /path/to/libllama.so      # Linux
hugind config defaults --hf-token hf_xxx               # for gated HF repos
```

## Quickstart
1. **Download a model**  
   ```bash
   hugind model add TheBloke/Mistral-7B-Instruct-v0.2-GGUF
   ```
   Models live in `~/.hugind/<user>/<repo>/*.gguf`.

2. **Generate a config** (hardware probe + wizard)  
   ```bash
   hugind config init my-chat-bot
   ```
   - Picks a preset (`metal_unified`, `cuda_dedicated`, `cpu_only`) and suggests context length based on RAM and model size  
   - Finds sibling vision projectors (`mmproj`) and detects chat format (`auto/chatml/gemma/alpaca/harmony`)  
   - Saves to `~/.hugind/configs/my-chat-bot.yml`

3. **Start the server**  
   ```bash
   hugind server start my-chat-bot
   # optional: override port or lib path
   # hugind server start my-chat-bot --port 9090 --lib /custom/libllama.dylib
   ```
   Outputs URLs for `/health`, `/v1/chat/completions`, and `/v1/models`.

4. **Call the API** (OpenAI-compatible)  
   ```bash
   curl http://localhost:8080/v1/chat/completions \
     -H "Content-Type: application/json" \
     -d '{
       "model": "my-chat-bot",
       "user": "alice",
       "messages": [{"role": "user", "content": "Hello!"}],
       "stream": true
     }'
   ```
   Use a stable `"user"` id to reuse the cached context without resending history.

## CLI Reference
- `hugind model add <user/repo>` – interactive GGUF downloader from Hugging Face  
- `hugind model list` / `show <user/repo>` / `remove <user/repo>` – manage local weights
- `hugind config info` – probe hardware and recommend a preset
- `hugind config init <name>` – create a YAML config via wizard (stores in `~/.hugind/configs/`)
- `hugind config list` / `remove <name>` – manage saved configs
- `hugind config defaults --lib … --hf-token …` – global defaults in `~/.hugind/settings.yml`
- `hugind server list` – show configs and whether their ports are live
- `hugind server start <config>` – run the OpenAI-compatible server

## Architecture Highlights
- **Slots & eviction:** LRU slots in VRAM; inactive sessions spill to RAM, then archive to `sessions/*.bin` on disk with a background sweeper. Returning users reload instantly.  
- **Time-slicing:** Prioritizes single-user latency over continuous batching; each active user gets full compute during their turn.  
- **Config-driven:** YAML maps directly to `llama.cpp` parameters (model path, GPU offload, context, sampling, server host/port/API key).

## Directory Layout
- Configs: `~/.hugind/configs/*.yml`
- Global defaults: `~/.hugind/settings.yml`
- Models: `~/.hugind/<user>/<repo>/*.gguf`
- Vision/draft helpers: auto-detected next to the selected model when present

## Documentation
- `docs/USER.md` – overview and workflow
- `docs/MODEL.md` – model management commands and storage layout
- `docs/CONFIG.md` – config wizard, presets, and templates
- `docs/SERVER.md` – server architecture and API surface
- `docs/SLOT.md` – slot-based memory system and eviction strategy
- `docs/DEV.md` – notes for contributors


## Demo

```bash
(base) adel@192 homebrew-hugind % brew install hugind 
✔︎ JSON API formula.jws.json                                                                                                                                                          [Downloaded   31.7MB/ 31.7MB]
✔︎ JSON API cask.jws.json                                                                                                                                                             [Downloaded   15.0MB/ 15.0MB]
==> Fetching downloads for: hugind
✔︎ Formula hugind (0.1.2)                                                                                                                                                             [Verifying     4.8MB/  4.8MB]
==> Installing hugind from netdur/hugind
🍺  /opt/homebrew/Cellar/hugind/0.1.2: 16 files, 12.4MB, built in 1 second
==> Running `brew cleanup hugind`...
Disable this behaviour by setting `HOMEBREW_NO_INSTALL_CLEANUP=1`.
Hide these hints with `HOMEBREW_NO_ENV_HINTS=1` (see `man brew`).
(base) adel@192 homebrew-hugind % hugind --version
hugind version 0.1.2
(base) adel@192 homebrew-hugind % hugind config info
System Information
------------------
OS: macos Version 26.1 (Build 25B78)
Arch: arm64
CPU: Apple M1 Max
Cores: 10 physical / 10 logical
Memory: 32.0 GB
Disk: 1858.2 GB total / 1017.2 GB free
GPUs:
  - Apple M1 Max (Unknown VRAM)

Recommendation: metal_unified
(base) adel@192 homebrew-hugind % hugind model list 
No models found. Run "hugind model add <hf_repo>" to download one.
(base) adel@192 homebrew-hugind % hugind model add llmware/tiny-llama-chat-gguf
Fetched file list🔍                                                                                                                                                                                               
✔ Select files to download (Space to select, Enter to confirm): · tiny-llama-chat.gguf                                                                                                                            

Starting download for 1 file(s)...

Done.
^C
(base) adel@192 homebrew-hugind % hugind model list                            

Downloaded Repositories:
----------------------------------------
llmware/tiny-llama-chat-gguf

(base) adel@192 homebrew-hugind % hugind config init tiny-llama
Probing hardware... (this may take a moment)
System probe complete:
  CPU: Apple M1 Max (10c/10t)
  Memory: 32.0 GB
  GPUs: Apple M1 Max
Recommended preset: metal_unified
✔ Choose a hardware preset to apply · metal_unified                                                                                                                                                               
✔ Select a Model Repository · llmware/tiny-llama-chat-gguf                                                                                                                                                        
✔ Select the Model File · tiny-llama-chat.gguf                                                                                                                                                                    
✔ Select Chat Format Template · harmony                                                                                                                                                                           

🧠 Memory Analysis:
  System RAM: 32.0 GB
  Model Size: 0.6 GB
  Est. Max Context: ~30083 tokens
✔ Select Context Size (Ctx) · 16384 (Recommended)                                                                                                                                                                 

✔ Config written to /Users/adel/.hugind/configs/tiny-llama.yml
  • Preset: metal_unified
  • Model: ~/.hugind/llmware/tiny-llama-chat-gguf/tiny-llama-chat.gguf
  • Library: /opt/homebrew/Cellar/hugind/0.1.2/libexec/libmtmd.dylib
  • Context: 16384
(base) adel@192 homebrew-hugind % hugind config list           
Saved Configs:
- tiny-llama
(base) adel@192 homebrew-hugind % hugind server start tiny-llama
🚀 Initializing Hugind Server (tiny-llama)...
⚠️  Warning: Vision projector not found at . Vision will be disabled.
   → Model: /Users/adel/.hugind/llmware/tiny-llama-chat-gguf/tiny-llama-chat.gguf
   → Context: 16384 (Batch: 2048)
   → Architecture: 1 Workers / 4 Slots per worker
   → Deploying 1 engine instance(s)...
     ✓ Instance #1 ready

✅ Server listening at http://0.0.0.0:8080
   Local Health: http://127.0.0.1:8080/health
   OpenAI URL:   http://127.0.0.1:8080/v1
   Press Ctrl+C to stop.
```

on another terminal

```bash
(base) adel@192 hugind % curl -X POST http://127.0.0.1:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "tiny-llama",
    "messages": [{"role": "user", "content": "say hello"}]    
  }'
data: {"id":"chatcmpl-1764031008709","object":"chat.completion.chunk","created":1764031008,"model":"tiny-llama","choices":[{"index":0,"delta":{"content":"Ex"},"finish_reason":null}]}

data: {"id":"chatcmpl-1764031008716","object":"chat.completion.chunk","created":1764031008,"model":"tiny-llama","choices":[{"index":0,"delta":{"content":"pert"},"finish_reason":null}]}

data: {"id":"chatcmpl-1764031008721","object":"chat.completion.chunk","created":1764031008,"model":"tiny-llama","choices":[{"index":0,"delta":{"content":"|"},"finish_reason":null}]}

data: {"id":"chatcmpl-1764031008727","object":"chat.completion.chunk","created":1764031008,"model":"tiny-llama","choices":[{"index":0,"delta":{"content":"user"},"finish_reason":null}]}

data: {"id":"chatcmpl-1764031008732","object":"chat.completion.chunk","created":1764031008,"model":"tiny-llama","choices":[{"index":0,"delta":{"content":"|"},"finish_reason":null}]}

data: {"id":"chatcmpl-1764031008739","object":"chat.completion.chunk","created":1764031008,"model":"tiny-llama","choices":[{"index":0,"delta":{"content":"me"},"finish_reason":null}]}

data: [DONE]
```
