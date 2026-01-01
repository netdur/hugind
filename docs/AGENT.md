# 🦅 Hugind Agent Submodule: Implementation Plan

## 1. Vision & Philosophy
The **Agent Submodule** transforms `hugind` into a platform for autonomous local tasks.
*   **Architecture:** Client-Server. The Agent is a lightweight client that consumes the existing Hugind Inference Server.
*   **Extension Model:** Agents are discoverable, self-contained packages (like browser extensions).
*   **Security:** Logic runs in a **Dart Eval Sandbox**. No raw shell access is granted unless explicitly bridged by the Host.
*   **Zero-Overhead Discovery:** Agents find the model by reading your existing `~/.hugind/configs/*.yml` files.

---

## 2. Directory Structure
Agents reside in `~/.hugind/agents/`. Each agent is a folder containing its configuration, identity, and logic.

```text
~/.hugind/
├── configs/                  # (Existing) Server configurations
│   └── gemma-4b.yml          # Defines port: 8080
├── agents/                   # (New) Installed Agent Packages
│   └── git-committer/
│       ├── agent.yaml        # Manifest & Permissions
│       ├── system.md         # Personality / System Prompt
│       └── main.drt          # Sandboxed Logic Script
```

---

## 3. Configuration Contract

### 3.1 Agent Manifest (`agent.yaml`)
Defines the agent's identity and its dependencies on your infrastructure.

```yaml
name: "GitCommitter"
version: "1.0.0"
description: "Scans a folder and generates commits using AI."

# CONNECTION
# Maps to ~/.hugind/configs/gemma-4b.yml
# The CLI reads that file to find the port (e.g., 8080).
backend: "gemma-4b" 

# ENTRY POINT
# The Dart Eval script to execute.
entry_point: main.drt

# IDENTITY
system_prompt_path: system.md

# PERMISSIONS (The Sandbox Boundaries)
permissions:
  filesystem:
    allow_paths: ["{{args[0]}}"] # Only access the folder passed in CLI
  network:
    allow_hosts: ["localhost"]   # Only talk to the local API
```

---

## 4. Architecture: The Sandbox & Bridge

The Agent Logic (`main.drt`) runs in a Virtual Machine (`dart_eval`). It cannot touch the OS directly. The `hugind` binary acts as the **Host**, injecting specific capabilities (The Bridge).

### 4.1 The Bridge Capabilities
The Host injects a `context` map into the script with these tools:

1.  **`capabilities['llm']`**: An HTTP client pre-configured to talk to the `backend` defined in YAML. It handles the JSON payload for OpenAI-compatible chat.
2.  **`capabilities['sys']`**: A safe wrapper around `Process.run`. It enforces `permissions.filesystem`.
3.  **`capabilities['console']`**: Methods for `print`, `ask` (user input), and `confirm` (y/n).
4.  **`args`**: List of arguments passed from the CLI.

### 4.2 The Script Logic (`main.drt`)
```dart
dynamic main(Map<String, dynamic> context) async {
  // 1. Setup
  var targetDir = context['args'][0];
  var sys = context['capabilities']['sys'];
  var llm = context['capabilities']['llm'];

  // 2. Tool Execution (Git Diff)
  var diff = await sys.run('git', ['diff', '--staged'], workDir: targetDir);
  if (diff.isEmpty) return print("Nothing to commit.");

  // 3. Brain Execution (Inference)
  // The 'llm' object already knows the URL from the server config
  var msg = await llm.chat("Generate a commit message for:\n$diff");

  // 4. Human-in-the-loop
  if (await sys.confirm("Commit with message: '$msg'?")) {
    await sys.run('git', ['commit', '-m', msg], workDir: targetDir);
    print("Done.");
  }
}
```

---

## 5. Implementation Phases

### Phase 1: Logic & Discovery
**Goal:** Resolve the `backend` to a real URL.

*   **Task:** Create `AgentCommand` (subcommand `run`).
*   **Task:** Parse `agent.yaml` to find `backend: "gemma-4b"`.
*   **Task:** Reuse existing Config Parser to read `~/.hugind/configs/gemma-4b.yml`.
*   **Task:** Extract `server.port`. Construct URL: `http://localhost:8080`.
*   **Check:** Ping `http://localhost:8080/health`. If unreachable, fail gracefully ("Server not started").

### Phase 2: The Sandbox Engine
**Goal:** Integrate `dart_eval` to run code securely.

*   **Task:** Add `dart_eval` dependency.
*   **Task:** Create `SandboxCompiler`. It reads the `.drt` file as a string and compiles it to bytecode.
*   **Task:** Create the `Bridge` classes (`SysCapability`, `LlmCapability`) that wrap native Dart functions.

### Phase 3: Capability Injection
**Goal:** Pass the "Tools" into the Sandbox.

*   **Task:** Implement `LlmCapability`.
    *   Needs to automatically prepend the content of `system.md` to the chat history.
*   **Task:** Implement `SysCapability`.
    *   Needs to check `Directory(workDir)` against `permissions.filesystem` before running `Process.run`.

### Phase 4: CLI Experience
**Goal:** Polish the user interaction.

*   **Task:** `hugind agent list` - Scans the `agents/` directory.
*   **Task:** `hugind agent init <name>` - Generates a template agent package.

---

## 6. Example Workflow

1.  **User Starts Server:**
    ```bash
    hugind server start gemma-4b
    # Server running on port 8080
    ```

2.  **User Runs Agent:**
    ```bash
    hugind agent run git-committer ./my-project
    ```

3.  **Internal Flow:**
    *   CLI reads `agents/git-committer/agent.yaml`.
    *   Finds `backend: gemma-4b`.
    *   Reads `configs/gemma-4b.yml` -> Port 8080.
    *   Injects `LlmClient(http://localhost:8080)` into Sandbox.
    *   Runs `main.drt`.

4.  **Output:**
    ```text
    > Analyzing ./my-project...
    > Generated message: "feat: add user login"
    > Execute? [y/N]: y
    > Success.
    ```