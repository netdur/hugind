# Hugind Agent Architecture

The **Agent Submodule** transforms `hugind` into a platform for autonomous local tasks. It enables you to run secured, sandboxed Dart scripts that can interact with your local LLM server and your operating system.

## 1. Core Philosophy

*   **Architecture:** Client-Server. The Agent is a lightweight client that consumes the existing Hugind Inference Server.
*   **Extension Model:** Agents are discoverable, self-contained packages (like browser extensions) stored in `~/.hugind/agents/`.
*   **Security:** Logic runs in a **Dart Eval Sandbox**. No raw shell access is granted unless explicitly bridged by the Host.
*   **Zero-Overhead Discovery:** Agents find the model by reading your existing `~/.hugind/configs/*.yml` files.

---

## 2. Directory Structure

Agents reside in `~/.hugind/agents/`. Each agent is a folder containing its configuration, entry point, and assets.

```text
~/.hugind/
├── configs/                  # Server configurations
│   └── gemma-4b.yml          # Defines port: 8080
├── agents/                   # Installed Agent Packages
│   └── git-committer/
│       ├── agent.yaml        # Manifest & Permissions
│       ├── system.md         # Personality / System Prompt
│       └── main.dart         # Sandboxed Logic Script
```

---

## 3. The Agent Manifest (`agent.yaml`)

Every agent must have a manifest. This defines the **Security Boundary** and runtime requirements.

```yaml
name: "GitCommitter"
version: "1.0.0"
description: "Scans a folder and generates commits using AI."

# CONNECTION
# Maps to ~/.hugind/configs/gemma-4b.yml
# The CLI reads that file to find the port (e.g., 8080).
backend: "gemma-4b" 

# ENTRY POINT
# The Dart source file to execute.
entry_point: main.dart

# PERMISSIONS (The Sandbox Boundaries)
permissions:
  # 📂 Filesystem
  filesystem:
    # If allowed_paths is empty or omitted, access is restricted to the Agent's own directory.
    # You can add arbitrary paths here, but usually agents work on CWD or passed args.
    allowed_paths: [] 

  # 🌐 Network
  network:
    allowed_domains:
      - "api.github.com"
      - "localhost"

  # 💻 Shell (Default: false)
  # Allows executing arbitrary system commands. USE WITH CAUTION.
  shell:
    allow: false 

# 🔌 DEPENDENCIES (MCP)
dependencies:
  mcp:
    - name: "filesystem"
    - name: "github"
```

---

## 4. The Sandbox & Bridge

The Agent Logic (`main.dart`) runs in a Virtual Machine (`dart_eval`). It cannot touch the OS directly. The `hugind` binary acts as the **Host**, injecting specific capabilities (The Bridge).

### 4.1 The Bridge Capabilities
The Host injects a `context` map into the script with these tools:

1.  **`capabilities['llm']`**: An HTTP client pre-configured to talk to the `backend` defined in YAML. It handles the JSON payload for OpenAI-compatible chat.
2.  **`capabilities['sys']`**: A safe wrapper around `Process.run` and Console I/O. It enforces `permissions.filesystem` and `permissions.shell`.
3.  **`capabilities['net']`**: A restricted HTTP client for external API calls. Enforces `permissions.network`.
4.  **`capabilities['mcp']`**: Access to Model Context Protocol tools.
5.  **`args`**: List of arguments passed from the CLI.

### 4.2 The Script Logic (`main.dart`)
```dart
dynamic main(Map<String, dynamic> context) async {
  // 1. Setup
  var args = context['args'];
  var sys = context['capabilities']['sys'];
  var llm = context['capabilities']['llm'];

  // 2. Interaction
  sys.print("Hello! I am an agent.");
  
  if (await sys.confirm("Shall I proceed?")) {
     var response = await llm.chat("Hello there!");
     sys.print(response);
  }
}
```

---

## 5. Execution Flow

1.  **User Trigger:** `hugind agent run git-committer ./my-project`
2.  **Manifest Parse:** CLI reads `agent.yaml`.
3.  **Backend Resolution:** CLI finds `backend: gemma-4b`, looks up `configs/gemma-4b.yml`, and resolves `localhost:8080`.
4.  **Security Check:** CLI builds the Sandbox with only the allowed permissions (Network domains, Filesystem paths).
5.  **Injection:** The Bridge capabilities are injected into the `dart_eval` runtime.
6.  **Run:** The `main.dart` script executes within the secure bounds.