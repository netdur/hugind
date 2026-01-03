# Hugind 🦅

> **Native, Stateful, High-Performance Inference Server for Local LLMs.**

Hugind turns your local machine into a production-grade AI inference backend. It wraps `llama.cpp` with a smart management layer, providing an OpenAI-compatible API that features **automatic state persistence**, efficient **resource management**, and a **developer-friendly CLI**.

Powered by [llama_cpp_dart](https://github.com/netdur/llama_cpp_dart).

---

## ⚡️ Key Features

*   **🚀 Native Performance**: Optimized presets for **Apple Silicon (Metal)**, **CUDA**, and CPU-only environments. Tuned for maximum throughput and low latency.
*   **🧠 Stateful Memory System**: A unique **3-tier architecture** (VRAM → RAM → Disk) that persists user sessions. 
    *   **Hot**: Active slots stay in VRAM for instant access.
    *   **Warm**: Idle sessions map to system RAM when VRAM is full.
    *   **Cold**: Long-term storage hibernates to disk, surviving server restarts.
*   **🛠️ "Smart" CLI**: No more 50-flag command lines. Use the **interactive wizard** to probe your hardware, calculate safe context limits, and generate clean YAML configs.
*   **🔌 OpenAI Compatible**: Drop-in replacement for your existing apps. Supports `/v1/chat/completions` (with streaming) and `/v1/models`.
*   **👥 True Multi-Tenancy**: Designed to handle 100+ concurrent user sessions efficiently using time-slicing and LRU eviction.
*   **🤖 Secure Agent Sandbox**: Run autonomous Dart agents that can interact with your system and LLMs safely. Features a permission-based capabilities system and full isolation.

---

## 📦 Installation

### Option 1: Homebrew (macOS)
The easiest way to get started on macOS.
```bash
brew install hugind
```

### Option 2: Build from Source
Requires the Dart SDK (3.0+).
```bash
git clone https://github.com/netdur/hugind.git
cd hugind
bash build.sh
export PATH="$PATH:$(pwd)/bin"
```

### ⚙️ One-Time Setup
Hugind needs to know where your `libllama` shared library is.
```bash
# macOS
hugind config defaults --lib /path/to/libllama.dylib

# Linux
hugind config defaults --lib /path/to/libllama.so

# Optional: Set token for gated Hugging Face models
hugind config defaults --hf-token hf_your_token_here
```

---

## 🚀 Quick Start

### 1. Download a Model
Use the interactive downloader to fetch GGUF files directly from Hugging Face.
```bash
hugind model add google/gemma-2-9b-it-GGUF
# Follow the prompts to select quantization (e.g., Q4_K_M)
```

### 2. Create a Config
Run the hardware probe wizard. It detects your GPU/RAM and recommends settings to prevent OOM crashes.
```bash
hugind config init my-assistant
# 1. Select Preset (e.g., metal_unified)
# 2. Select Model (e.g., gemma-2-9b)
# 3. Select Chat Format (e.g., gemma)
# 4. Confirm Context Size (auto-calculated)
```

### 3. Start the Server
Launch your inference engine.
```bash
hugind server start my-assistant
```
You'll see:
```text
✅ Server listening at http://0.0.0.0:8080
   Local Health: http://127.0.0.1:8080/health
   OpenAI URL:   http://127.0.0.1:8080/v1

### 4. Run an Agent
Experience autonomous interaction.
```bash
hugind agent run joke-bot
# Interact with the JokeBot directly in your terminal.
```


---

## 💬 Usage

### OpenAI-Compatible API
Interact with Hugind using `curl`, Python `openai` lib, or any standard tool.

```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "my-assistant",
    "messages": [
      {"role": "user", "content": "Hello, world!"}
    ],
    "stream": true
  }'
```

### ✨ The "Stateful" Advantage
Unlike standard servers, Hugind can **remember** context without you re-sending it. Use the `X-Session-ID` header to resume a conversation instantly.

**Request 1 (Session A):** "My name is Adel."
```bash
curl -H "X-Session-ID: session-a" ... -d '{"messages": [{"role": "user", "content": "My name is Adel."}]}'
```

**Request 2 (Session A):** "What is my name?"
```bash
# No need to send previous messages!
curl -H "X-Session-ID: session-a" ... -d '{"messages": [{"role": "user", "content": "What is my name?"}]}'
```
**Result:** "Your name is Adel."

*(Even if Session A was evicted from VRAM to make room for others, Hugind restores it for Request 2 silently.)*

---



## 🤖 Autonomous Agents

Hugind runs pure Dart agents in a secure, sandboxed environment. Agents can reason, plan, and interact with the host system via capabilities (like `sys.run`, `sys.readInput`) while adhering to strict permission boundaries.

### Example: CLI Navigator
The `cli-navigator` agent converts natural language into shell commands. It features **Smart Safety Checks**: safe commands (read-only) run automatically, while dangerous ones (state-changing) require user confirmation.

`bin/hugind agent run examples/agents/cli-navigator`

```text
ℹ️  Agent "examples/agents/cli-navigator" connecting to http://0.0.0.0:8080 (gemma-4b)...
🚀 Running Agent...
CLI Navigator ready. Describe what you want, or type "exit" to quit.

> list file in folder and sort by largest 
Thought: Turn the request into a safe single command.
Action: ls -S
Observation:
hugind-macos-arm64.tar.gz
pubspec.lock
README.md
...

> what is my adb version 
Thought: Turn the request into a safe single command.
Action: adb --version
Observation:
Android Debug Bridge version 1.0.41
Version 36.0.0-13206524
...

> exit
Goodbye.
✅ Agent finished.
```

---

## 🎛️ Configuration

Configs live in `~/.hugind/configs/*.yml`. They are clean, readable, and hardware-aware.

```yaml
model:
  path: /Models/gemma-2-9b.gguf
  gpu_layers: 99        # Full GPU offload
  use_mmap: true

context:
  size: 8192            # Context window
  flash_attention: true # Optimized kernels

server:
  host: 0.0.0.0
  port: 8080
  api_key: "my-secret"  # Optional protection
```

---

## 📚 Documentation

Detailed guides for every part of the system:
*   [**User Guide**](docs/USER.md): In-depth workflow and concepts.
*   [**Server Architecture**](docs/SERVER.md): Deep dive into the engine and API.
*   [**API Reference**](docs/API.md): Full endpoint compatibility table.
*   [**Config Guide**](docs/CONFIG.md): Presets, templates, and parameters.
*   **[Agent Guide](docs/AGENT.md)**: Architecture and capabilities of the Agent system.
*   **[Agent Development](docs/AGENT_DEV.md)**: How to write and deploy your own agents.

---

## 🤝 Contributing

Contributions are welcome! Please check [docs/DEV.md](docs/DEV.md) for build instructions and architecture notes.

## 📄 License

MIT
