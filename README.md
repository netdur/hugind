# Hugind

Local-only LLM inference and agent system. Uses `llama.cpp` as the engine, Rust as the implementation language. Aims to keep agents under control.

## Server

OpenAI-compatible HTTP API backed by `llama.cpp`:

* Continuous batching with stateful sessions
* Context shifting (for supported models)
* 3-tier KV cache: VRAM, RAM, disk (uses the fastest available; loading from disk can be faster than re-processing a large history)
* Multimodal (image + text)
* Embeddings
* Authentication + streaming

Each model runs with its own config, OS process, and port. A model can have several configs depending on the task.

## Agent

Sandboxed agent execution with process isolation. Each agent runs in its own OS process.

Two runtimes:

* JavaScript (`rquickjs`)
* WASM (Wasmtime) for modules compiled from any language

Permission model (declared in `agent.yaml`):

* **Network**: allowlist by domains/IPs, DNS-based rules, private network blocking
* **Filesystem**: scoped read/write/create/delete, allowed/denied paths, WASM mounts
* **Shell**: allowlist/blocklist, OS sandboxing (macOS sandbox-exec), timeouts, output limits
* **Resources** (WASM): memory and CPU limits

Other:

* MCP server support (declared per-agent under `dependencies.mcp`)
* Agent install from git repos, local paths, or URLs (`hugind agent install <path>`)
* Session logging under `~/.hugind/logs/agents/`

## Model

* Download GGUF models from Hugging Face (with resume and SHA256 integrity verification)
* Concurrent downloads
* Local storage under `~/.hugind/models/{user}/{repo}/`

## Config

* Hardware auto-detection (CPU, RAM, GPU)
* Auto-fit support (engine adjusts context to fit available memory)
* Hardware-aware defaults (GPU layers, threads, flash attention, KV offload)

## Stdio bridge

NDJSON + MCP protocol for desktop/app integration (`hugind stdio`).

## Quick start

### Install
```bash
brew install hugind
hugind --version
```

### Set Hugging Face token (if needed)
```bash
hugind config defaults --hf-token <your_token>
```

### Download a model
```bash
hugind model add google/gemma-3-4b-it-qat-q4_0-gguf
```

### Create a config
```bash
hugind config init gemma-4b
```
The wizard will:
- probe your hardware (CPU, RAM, GPU)
- let you select a downloaded model
- ask whether to enable auto-fit or choose a context size manually

Config is written to `~/.hugind/configs/gemma-4b.yml`.

### Start the server
```bash
hugind server start gemma-4b
```

### Send a request
```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemma-4b",
    "messages": [{ "role": "user", "content": "who are you?" }]
  }'
```

See [HTTP API](docs/http_api.md) for streaming, multimodal, sessions, embeddings, and state management.

## Agent demo

```bash
hugind agent install https://github.com/netdur/hugind/tree/main/agent/audit

Requested permissions:
- Network access: No
- File access: Yes (actions: read; can access outside agent folder)
- Run system commands: No
> Grant these permissions and install this agent? Yes
Installed agent 'audit' to /Users/adel/.hugind/agents/audit
```

Run the audit agent against another agent's code:
```bash
hugind agent run audit agent/ocr

Checking server health at http://127.0.0.1:8080/v1/monitor...
Server is up. Starting agent...
{
  "Alignment": "PASS",
  "Security": "PASS",
  "Confidence": "high"
}
```

Run an agent with arguments:
```bash
hugind agent run agent/ocr --image /path/to/image.jpg --prompt "read the title"

{"blocks": [{"block_type": "text", "text": "...", "bbox_2d": [171, 133, 783, 223]}]}
```

Agent runs are logged:
```
[2026-02-10T12:29:33.526Z] agent.run.start name=ocr entry=main.js
[2026-02-10T12:29:33.530Z] host.fs.read_bytes path=/path/to/image.jpg
[2026-02-10T12:29:35.014Z] host.llm.chat_stream input=object messages=Some(1)
[2026-02-10T12:29:41.198Z] agent.run.complete status=ok
```

## Docs

* [Agent Manifest](docs/agent_manifest.md) -- `agent.yaml` format, permissions, dependencies
* [CLI Overview](docs/cli.md) -- top-level commands
* [CLI: agent](docs/cli_agent.md) -- run, install, list, remove
* [CLI: config](docs/cli_config.md) -- list, validate, init, defaults, info
* [CLI: model](docs/cli_model.md) -- add, show, remove, migrate
* [CLI: server](docs/cli_server.md) -- start, list, stop
* [HTTP API](docs/http_api.md) -- endpoints, request/response shapes, sessions
* [JavaScript Runtime](docs/js_runtime.md) -- execution model, globals, capabilities
* [Server Config](docs/server_config.md) -- config sections, defaults, path resolution
* [Stdio Bridge](docs/stdio_bridge.md) -- NDJSON + MCP protocol reference
* [Testing Guide](docs/testing.md) -- local and CI test commands
* [WASM Runtime](docs/wasm_runtime.md) -- execution lifecycle, hostcalls, resource limits
* [WASM SDK (AssemblyScript)](docs/wasm_sdk_assemblyscript.md) -- hostcall wrappers

## License

MIT
