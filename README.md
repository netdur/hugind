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
