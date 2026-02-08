# Hugind 🦅

**The local-first, stateful inference engine and sandboxed agent runtime.**

Hugind is a high-performance system built in **Rust** designed specifically for consumer hardware. Unlike cloud-native solutions, Hugind focuses on maximizing local resources through aggressive caching, process isolation, and a specialized 3-tier memory architecture.

## 🚀 Key Philosophy

* **Local-Only:** Engineered for your machine, not the cloud.
* **Performance First:** Native `llama.cpp` bindings with continuous batching.
* **Secure by Design:** Every model and every agent runs in its own isolated OS process.
* **Stateful:** Stop recomputing prompts. Hugind remembers the context so you don't have to wait.

---

## 🧠 Inference Server

The core server leverages a custom **llama.cpp** binding to provide a robust, OpenAI-compatible API.

* **Continuous Batching:** Process multiple requests simultaneously without blocking.
* **3-Tier Cache (VRAM → RAM → Disk):** Hugind intelligently moves session data between tiers. Loading a massive context from Disk is often faster than re-processing tokens.
* **Session Branching:** Instantly fork a "base" session (like a large FAQ or system prompt) into parallel, independent workers.
* **Smart Context Management:** Uses context shifting and request chunking to keep GPU memory usage predictable and bounded.
* **Multi-Modal:** Full support for image-based reasoning.

---

## 🛡️ Sandboxed Agent Runtime

Hugind isn't just an LLM server; it’s a secure execution environment for AI agents.

| Feature | JavaScript (`rquickjs`) | WASM (`Wasmer`) |
| --- | --- | --- |
| **Isolation** | OS Process | OS Process + Sandbox |
| **Speed** | Ultra-lightweight | Near-native |
| **Language** | JS/TS | Any (C++, Rust, Go, etc.) |

### Fine-Grained Controls

* **Network:** Strict allowlists for IPs and domains (supports wildcards `*`).
* **Filesystem:** Scoped access with explicit read/write mounts.
* **Resource Limits:** Define strict RAM and CPU caps for WASM modules.
* **Permission Prompting:** The `hugind agent install url` informs users exactly what an agent is requesting before it install.

---

## 🛠️ Architecture: The Process Model

Unlike monolithic servers, Hugind treats stability as a priority:

* **Model Isolation:** Each model runs in its own OS process with a dedicated port. If one model crashes, the system stays up.
* **Agent Isolation:** Each agent is partitioned away from the core server, communicating only through defined APIs.
* **Client-Facing:** The server is designed to sit behind an application layer—not to be exposed directly to the open web.

---

## 💻 Tooling & CLI

Hugind comes with a suite of tools to manage your local AI stack:

* **Model CRUD:** Download and manage GGUF models directly from Hugging Face.
* **Hardware Probe:** Auto-configures Hugind based on your available VRAM and CPU cores.
* **Chat CLI:** A "ChatGPT-style" interface for immediate testing and validation of your deployment.

## 📄 License

MIT