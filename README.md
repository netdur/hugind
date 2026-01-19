# Hugind 🦅

> **Native, Stateful, High-Performance Inference Server & Agent Runtime.**

Hugind turns your local machine into a production-grade AI backend. It wraps `llama.cpp` with a smart management layer, providing an OpenAI-compatible API that features **automatic state persistence**, efficient **resource management**, and a secure **sandboxed agent runtime**.

Powered by [llama_cpp_dart](https://github.com/netdur/llama_cpp_dart).

---

## ⚡️ Key Features

*   **🚀 Native Performance**: Optimized presets for **Apple Silicon (Metal)**, **CUDA**, and CPU-only environments.
*   **🧠 Stateful Memory (3-Tier Architecture)**: Powered by `llama_cpp_dart`'s `LlamaService`, Hugind intelligently manages user sessions:
    *   **Hot**: Active slots stay in VRAM for instant access.
    *   **Warm**: Idle sessions map to system RAM when VRAM is full.
    *   **Cold**: Long-term storage hibernates to disk (see `server.session_home`), surviving server restarts.
*   **🛡️ Secure Agent Runtime**: Run community agents safely. Hugind treats agents like **browser extensions**—sandboxed scripts with strict, manifest-based permissions (`agent.yaml`).
*   **🔌 MCP Client**: Native support for the **Model Context Protocol**. Connect agents to external tools (GitHub, Databases, Filesystem) via standard MCP servers.
*   **👁️ Multimodal Vision**: Native support for image inputs. Run models like `Llava` or `Moondream` via the OpenAI Vision API.
*   **🛠️ "Smart" CLI**: An interactive hardware probe that calculates safe context limits and generates OOM-proof configs.
*   **💬 Interactive Chat**: Built-in terminal workspace to chat with your models natively, featuring persistent sessions and slash commands.

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

### ⚙️ One-Time Setup (Optional)
**Note:** Hugind ships with a bundled `llama.cpp` runtime. You only need to configure this if you are a **power user** running a custom build of the library.

Link your custom `llama.cpp` library:
```bash
# macOS
hugind config defaults --lib /path/to/custom/libllama.dylib

# Linux
hugind config defaults --lib /path/to/custom/libllama.so
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

## 💬 Interactive Chat

Hugind includes a built-in terminal client that supports state persistence.

```bash
# Start a new chat (wizard)
hugind chat

# Start a specific chat session
hugind chat start my-assistant

# Resume a previous session
hugind chat resume <session-id>
```

inside the chat:
*   **Persistent State**: Sessions are saved to disk automatically.
*   **Hibernation**: When you exit (Ctrl+C), the session hibernates. Resuming is instant.
*   **Slash Commands**: Use `/help`, `/image`, `/text`, `/fork`, `/clear`, `/exit`.

---

## 🤖 The Agent Runtime (Sandboxed)

Hugind features a secure, **interpreted runtime** for AI Agents. 

Unlike other frameworks that might require compilation or external dependencies, Hugind Agents are pure **Dart source files** (`.dart`) that run directly. The Hugind binary ships with a **custom-patched version of the `dart_eval` runtime**, allowing it to dynamically consume and execute these scripts in a sandboxed environment.

### Installing an Agent
Agents are installed like plugins. Hugind analyzes the `agent.yaml` and warns you about permissions.

```bash
hugind agent install /path/to/stock-analyst
```

You can also install from a GitHub tree URL:

```bash
hugind agent install https://github.com/user/repo/tree/main/agent-folder
```

**The Security Check:**
```text
📦 Installing 'stock-analyst'...
   • Backend URL: http://127.0.0.1:8080/v1
   • Model: gemma-2-9b-it
   • 🌐 Network: api.stockdata.org, finance.yahoo.com
   • 📂 Filesystem: Read=✅, Write=✅
     Allowed paths: $HOME/Downloads, ./workspace
   • 💻 Shell (whitelist): ls, date
   • 🔌 MCP (required): postgres-client
   • 🔌 MCP (optional): github
   • 🔧 Required env: STOCK_API_KEY

This is an all-or-nothing permission grant. Accept? [y/N]
```

### Running an Agent
Once installed, agents run in a sandboxed runtime, connecting to the local inference server.

```bash
hugind agent run stock-analyst
```

---

## 🛡️ Agent Security Model (`agent.yaml`)

Every agent must have a manifest. This defines the **Security Boundary**.

```yaml
name: "stock-analyst"
version: "1.0.0"
description: "Fetches live stock data and generates a PDF report."
entry_point: "main.dart"

# ⚠️ COMPATIBILITY
hugind_version: ">=0.6.0"

# 🔗 BACKEND CONNECTION
backend:
  url: "http://127.0.0.1:8080/v1"
  config: "gemma-2-9b-it"
  session:
    mode: "fresh"                  # stateless | fresh | resume
    id: "stock-analyst"

# 🛡️ PERMISSIONS
permissions:
  # 🌐 Network
  network:
    allow: true
    allowed_domains:
      - "api.stockdata.org"
      - "finance.yahoo.com"

  # 📂 Filesystem
  filesystem:
    read: true
    write: true
    allowed_paths:
      - "$HOME/Downloads"
      - "./workspace"

  # 💻 Shell (Default: false)
  shell:
    allow: true
    whitelist:
      - "ls"
      - "date"

# 🔌 DEPENDENCIES (MCP)
dependencies:
  mcp:
    - name: "postgres-client"
      required: true
    - name: "github"
      required: false

# 🔧 ENV
env:
  - name: "STOCK_API_KEY"
    required: true
```

---

## 🔌 Model Context Protocol (MCP)

Hugind acts as an **MCP Client**. This allows your agents to use standard tools (like reading Git repos, querying Postgres) without the agent developer needing to write that logic.

1.  **Configure MCP Servers** in `<data_home>/settings.yml` (see `docs/cli.md` for path resolution):
    ```yaml
    mcp_servers:
      filesystem:
        command: "npx"
        args: ["-y", "@modelcontextprotocol/server-filesystem", "/Users/me/projects"]
    ```
2.  **Agents consume tools**: The agent script simply asks `sys.tools.list()` and Hugind handles the secure connection to the MCP server.

---

## 💬 API Usage

### 👁️ Vision (Multimodal)
Analyze images using models like `Llava` (local paths or data URLs only).
```bash
curl http://localhost:8080/v1/chat/completions ... -d '{
  "model": "llava-v1.6",
  "messages": [
    {
      "role": "user",
      "content": [
        {"type": "text", "text": "What is in this image?"},
        {"type": "image_url", "image_url": {"url": "file:///Users/me/cat.jpg"}}
      ]
    }
  ]
}'
```

### 🧠 The "Stateful" Advantage (Context Caching)
Hugind persists context across requests. Use the `X-Session-ID` header to resume a conversation instantly, even if the server has handled other users in between.

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

## 🎛️ Configuration

Configs live in `<config_home>/configs/*.yml` (see `docs/cli.md` for how paths resolve). They are clean, readable, and hardware-aware.

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

*   [**Docs Index**](docs/README.md)
*   [**CLI**](docs/cli.md)
*   [**Models**](docs/model.md)
*   [**Config**](docs/config.md)
*   [**Server**](docs/server.md)
*   [**Chat**](docs/chat.md)
*   [**Agents**](docs/agent.md)
*   [**Agent Development**](docs/agent_dev.md)
*   [**API Reference**](docs/api.md)
*   [**Developer Guide**](docs/developer.md)

## 🤝 Contributing

Contributions are welcome! Please check [docs/developer.md](docs/developer.md) for build instructions.

## 📄 License

MIT
