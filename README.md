# Hugind 🦅

> **Native, Stateful Inference Server & Sandboxed Agent Runtime.**

Hugind is a production-grade AI backend designed for stability and resource efficiency on consumer hardware. It wraps `llama.cpp` with a smart management layer, providing an OpenAI-compatible API that features **3-tier state persistence**, efficient **context branching**, and a **permission-gated agent runtime**.

Powered by [llama_cpp_dart](https://github.com/netdur/llama_cpp_dart).

---

## ⚡️ Key Features

*   **🧠 3-Tier Memory Architecture**: Intelligently manages KV-cache sessions to maximize VRAM:
    *   **Hot**: Active sessions stay in VRAM for zero-latency response.
    *   **Warm**: Idle sessions map to system RAM when VRAM is needed elsewhere.
    *   **Cold**: Long-term sessions hibernate to disk, surviving server restarts.
*   **🔱 Context Forking (`X-Session-Fork`)**: High-throughput batching for limited hardware. Clone a "template" KV-cache (e.g., a system prompt or a large document) instantly into multiple parallel branches. 
    *   *Benchmark: Successfully handles 25+ parallel requests on a 6GB RTX 2060.*
*   **🛡️ Secure Agent Runtime**: Run AI agents in a sandboxed, interpreted environment using a custom-patched `dart_eval`. Agents are gated by a manifest-based permission system (`agent.yaml`).
*   **🛠️ Self-Building Ecosystem**: Includes built-in agents that can **scaffold**, **refactor**, and **audit** other agents, creating a secure, self-healing development loop.
*   **🔌 MCP Client**: Native support for the Model Context Protocol. Connect agents to external tools (Postgres, GitHub, Filesystem) via standard MCP servers.
*   **👁️ Multimodal Vision**: Native support for image inputs (Llava, Moondream, Qwen-VL) via the OpenAI Vision API.
*   **📋 Hardware-Aware CLI**: A wizard-driven probe that calculates optimal `gpu_layers` and `context_size` to prevent Out-Of-Memory (OOM) crashes.

---

## 📦 Installation

### macOS (Homebrew)
```bash
brew tap netdur/hugind
brew install hugind
```

### Build from Source
Requires Dart SDK 3.0+.
```bash
git clone https://github.com/netdur/hugind.git
cd hugind
bash build.sh
export PATH="$PATH:$(pwd)/bin"
```

---

## 🚀 Quick Start

1.  **Download a Model**:
    ```bash
    hugind model add gemma-3-4b-it-qat-q4_0-gguf
    ```
2.  **Initialize Config**: (Detects VRAM and sets safe limits)
    ```bash
    hugind config init my-config
    ```
3.  **Start the Server**:
    ```bash
    hugind server start my-config
    ```

---

## 🤖 The Agent Lifecycle (Secure & Automated)

Hugind treats agents like browser extensions. They run as `.dart` scripts in a restricted runtime.

### 1. Build & Audit
You can use the built-in "Builder" agent to generate new tools and the "Audit" agent to verify their safety.

```bash
# Generate a math agent
hugind agent run builder examples/agents/math-tool

# Audit the code for security risks (e.g., shell injections)
hugind agent run audit examples/agents/math-tool
```

### 2. Permissions (`agent.yaml`)
Every agent is restricted by a manifest. If an agent isn't granted shell access, the runtime blocks the execution at the kernel level.

```yaml
permissions:
  network: { allow: false }
  filesystem: { read: true, allowed_paths: ["./data"] }
  shell: { allow: false }
```

---

## 💬 Stateful API Usage

Hugind eliminates the need to send the entire chat history with every request by maintaining state on the server.

### Basic Statefulness
Use `X-Session-ID` to resume a conversation. Hugind will automatically restore the KV-cache from RAM or Disk.

```bash
curl -H "X-Session-ID: session-123" \
     -d '{"messages": [{"role": "user", "content": "Remember my name is Adel."}]}' \
     http://localhost:8080/v1/chat/completions
```

### Efficient Branching (`X-Session-Fork`)
To process multiple different tasks from the same base context (like a large legal document or a complex system prompt) without re-processing:

1.  **Create a template session** (processed once).
2.  **Fork it** into new IDs:
```bash
# This request boots instantly from the 'legal-doc-template' cache
curl -H "X-Session-ID: worker-1" \
     -H "X-Session-Fork: legal-doc-template" \
     -d '{"messages": [{"role": "user", "content": "Summarize page 5."}]}' \
     http://localhost:8080/v1/chat/completions
```

---

## 🔌 Model Context Protocol (MCP)

Configure external tools in `settings.yml` to allow agents to interact with your local environment securely:

```yaml
mcp_servers:
  sqlite:
    command: "npx"
    args: ["-y", "@modelcontextprotocol/server-sqlite", "--db", "analytics.db"]
```

---

## 📚 Documentation

*   [**CLI Reference**](docs/cli.md) - Command usage and flags.
*   [**Agent Dev**](docs/agent_dev.md) - Writing sandboxed Dart agents.
*   [**Server Guide**](docs/server.md) - 3-Tier memory and fork logic.
*   [**API Spec**](docs/api.md) - Custom headers and OpenAI compatibility.

## 🤝 Contributing

Contributions are welcome. Please see [developer.md](docs/developer.md) for build instructions and the contribution guidelines.

## 📄 License

MIT