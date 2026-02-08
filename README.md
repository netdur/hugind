# Hugind 🦅

## Scope / positioning

* Local-only system (not intended to run in the cloud)
* Uses `llama.cpp` as the inference engine
* Aims to keep agents under control

## Implementation stack

* Rust (with a custom `llama.cpp` Rust binding)

Two execution runtimes:

* JavaScript runtime via `rquickjs`
* WASM runtime via Wasmtime (run WASM modules built from your favorite language)

## Server

* `llama.cpp` engine (including `llama.cpp` features)
* Continuous batching
* Large contexts are processed by splitting work into a fixed set of request chunks
* Context shifting
* Stateful sessions
* Designed for limited GPU memory: avoid recomputing full history and free GPU resources quickly

3-tier memory / cache:

* VRAM → RAM → disk
* Uses the fastest available tier; e.g. loading cache from disk can be faster than re-processing a large FAQ/history

Session branching:

* OpenAI-compatible HTTP API
* Authentication + streaming
* Embeddings supported
* Multimodal

Process model:

* Each model runs with its own config, OS process, and preconfigured port
* Application clients can probe ports or use predefined ports
* End users should not access the server directly (server is meant to sit behind an app, like a database)

## Agent

* Process isolation
* Each agent runs in its own OS process

Runtimes:

* JavaScript runtime (`rquickjs`)
* WASM runtime (Wasmtime) for running modules compiled from any language

Network access control (allowlist-based):

* Allowlist by IPs and domains
* Supports wildcards (`*`) and DNS-based rules

Filesystem access control (fine-grained):

* Scoped access with explicit read/write permissions
* WASM: filesystem mounts supported

Shell / OS access control (allowlist + sandboxing):

* Allowlist-based shell access with OS sandboxing
* WASM: RAM and CPU usage limitations
* Environment variables supported

Other:

* MCP support
* Agent install CLI (`hugind agent install <path>`)
* Informs the user of requested permissions and supports granting permissions explicitly

## Model

* Download models from Hugging Face
* Management for installed models

## Config

* Hardware probing for auto-configuration

## Chat (CLI)

* “ChatGPT-style” CLI for quick testing and deployment validation
* Basic administration features

## License

MIT
