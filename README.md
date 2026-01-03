# Hugind 🦅

> **Native, Stateful, High-Performance Inference Server & Agent Runtime.**

Hugind turns your local machine into a production-grade AI backend. It wraps `llama.cpp` with a smart management layer, providing an OpenAI-compatible API that features **automatic state persistence**, efficient **resource management**, and a secure **sandboxed agent runtime**.

Powered by [llama_cpp_dart](https://github.com/netdur/llama_cpp_dart).

---

## ⚡️ Key Features

*   **🚀 Native Performance**: Optimized presets for **Apple Silicon (Metal)**, **CUDA**, and CPU-only environments.
*   **🧠 Stateful Memory (3-Tier Architecture)**: A unique system that persists user sessions to manage context limits:
    *   **Hot**: Active slots stay in VRAM for instant access.
    *   **Warm**: Idle sessions map to system RAM when VRAM is full.
    *   **Cold**: Long-term storage hibernates to disk, surviving server restarts.
*   **🛡️ Secure Agent Runtime**: Run community agents safely. Hugind treats agents like **browser extensions**—sandboxed scripts with strict, manifest-based permissions (`agent.yaml`).
*   **🔌 MCP Client**: Native support for the **Model Context Protocol**. Connect agents to external tools (GitHub, Databases, Filesystem) via standard MCP servers.
*   **👁️ Multimodal Vision**: Native support for image inputs. Run models like `Llava` or `Moondream` via the OpenAI Vision API.
*   **🛠️ "Smart" CLI**: An interactive hardware probe that calculates safe context limits and generates OOM-proof configs.

---

## 📦 Installation

### Option 1: Homebrew (macOS)
```bash
brew tap netdur/hugind
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
Link your native `llama.cpp` library:
```bash
# macOS
hugind config defaults --lib /path/to/libllama.dylib

# Linux
hugind config defaults --lib /path/to/libllama.so
```

---

## 🚀 Quick Start

### 1. Download a Model
Fetch GGUF files directly from Hugging Face.
```bash
hugind model add google/gemma-2-9b-it-GGUF
```

### 2. Create a Config
Run the hardware probe wizard. It detects your VRAM and auto-calculates context limits.
```bash
hugind config init my-assistant
# Follow the prompts to select presets (e.g., metal_unified)
```

### 3. Start the Server
```bash
hugind server start my-assistant
```
You'll see:
```text
✅ Server listening at http://0.0.0.0:8080
   Local Health: http://127.0.0.1:8080/health
   OpenAI URL:   http://127.0.0.1:8080/v1
```

---

## 🤖 The Agent Runtime (Sandboxed)

Hugind features a secure, **interpreted runtime** for AI Agents. 

Unlike other frameworks that might require compilation or external dependencies, Hugind Agents are pure **Dart source files** (`.dart`) that run directly. The Hugind binary ships with a **custom-patched version of the `dart_eval` runtime**, allowing it to dynamically consume and execute these scripts in a sandboxed environment.

### Installing an Agent
Agents are installed like plugins. Hugind analyzes the `agent.yaml` and warns you about permissions.

```bash
hugind agent install netdur/stock-analyst
```

**The Security Check:**
```text
📦 Installing 'stock-analyst'...
⚠️  PERMISSIONS REQUESTED:
   • 🌐 Network: api.stockdata.org, finance.yahoo.com
   • 📂 Filesystem: Workspace Only (Safe)
   • 🔌 MCP: Requires 'filesystem' tool

Do you accept? [y/N]
```

### Running an Agent
Once installed, agents run in a dedicated process, connecting to the local inference server.

```bash
hugind agent run stock-analyst
```

---

## 🛡️ Agent Security Model (`agent.yaml`)

Every agent must have a manifest. This defines the **Security Boundary**.

```yaml
name: "stock-analyst"
version: "1.0.0"
entry_point: "main.dart"

# 🛡️ PERMISSIONS
permissions:
  # 🌐 Network: Whitelist specific domains only
  network:
    allowed_domains:
      - "api.stockdata.org"
      - "finance.yahoo.com"

  # 📂 Filesystem: Define read/write scope
  filesystem:
    read: true
    write: true
    # If allowed_paths is empty, access is restricted to the Agent's Workspace only.
    allowed_paths: [] 

  # 💻 Shell: Blocked by default
  shell:
    allow: false 

# 🔌 DEPENDENCIES (Model Context Protocol)
dependencies:
  mcp:
    - name: "filesystem" # Requires a local MCP server
      required: true

# 🔧 CONFIGURATION
env:
  - name: "STOCK_API_KEY"
    required: true
```

---

## 🔌 Model Context Protocol (MCP)

Hugind acts as an **MCP Client**. This allows your agents to use standard tools (like reading Git repos, querying Postgres) without the agent developer needing to write that logic.

1.  **Configure MCP Servers** in `~/.hugind/config.yaml`:
    ```yaml
    mcp_servers:
      filesystem:
        command: "npx"
        args: ["-y", "@modelcontextprotocol/server-filesystem", "/Users/me/projects"]
    ```
2.  **Agents consume tools**: The agent script simply asks `sys.tools.list()` and Hugind handles the secure connection to the MCP server.

---

## 💬 API Usage

### OpenAI-Compatible Chat
```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "my-assistant",
    "messages": [
      {"role": "user", "content": "Hello!"}
    ]
  }'
```

### 👁️ Vision (Multimodal)
Analyze images using models like `Llava`.
```bash
curl http://localhost:8080/v1/chat/completions ... -d '{
  "model": "llava-v1.6",
  "messages": [
    {
      "role": "user",
      "content": [
        {"type": "text", "text": "What is in this image?"},
        {"type": "image_url", "image_url": {"url": "https://..."}}
      ]
    }
  ]
}'
```

### 🧠 The "Stateful" Advantage (Context Caching)
Hugind persists context across requests. Use the `X-Session-ID` header to resume a conversation instantly, even if the server has handled other users in between.

```bash
# Request 1 (Context is processed and cached)
curl -H "X-Session-ID: session-a" ... -d '...'

# Request 2 (Zero prompt processing time)
curl -H "X-Session-ID: session-a" ... -d '...'
```

---

## 🎛️ Configuration

Configs live in `~/.hugind/configs/*.yml`. They are clean, readable, and hardware-aware.

```yaml
model:
  path: /Models/gemma-2-9b.gguf
  gpu_layers: 99        # Full GPU offload

context:
  size: 8192            # Context window
  flash_attention: true # Optimized kernels

server:
  host: 0.0.0.0
  port: 8080
```

---

## 📚 Documentation

*   [**User Guide**](docs/USER.md): In-depth workflow and concepts.
*   [**Agent Development**](docs/AGENT_DEV.md): How to write secure scripts and manifests.
*   [**Server Architecture**](docs/SERVER.md): Deep dive into the 3-Tier memory system.
*   [**API Reference**](docs/API.md): Full endpoint compatibility table.

## 🤝 Contributing

Contributions are welcome! Please check [docs/DEV.md](docs/DEV.md) for build instructions.

## 📄 License

MIT