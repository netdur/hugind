# Interactive Chat (Workspace)

Hugind ships with a powerful terminal-based chat client that leverages the server's stateful capabilities. It is designed to be a persistent workspace rather than just a simple REPL.

## Quick Start

```bash
# Open the Interactive Wizard
hugind chat
```

This command checks for existing sessions. If none exist, it prompts you to start a new one using your saved configurations.

## Subcommands

### 1. `hugind chat start <config_name>`
Bypasses the wizard and immediately starts a new session using the specified model configuration.

```bash
hugind chat start my-coding-assistant
```

### 2. `hugind chat resume <session_id>`
Resumes a specific session by its ID. You can find IDs using the `list` command.

```bash
hugind chat resume 550e8400-e29b
```

### 3. `hugind chat list`
Displays all stored sessions, their last active time, and the model used.

```bash
hugind chat list
```

**Output:**
```text
ID              LAST ACTIVE   TITLE
550e8400-e29b   5m ago        my-coding-assistant (gemma-2b)
a1b2c3d4-e5f6   2d ago        roleplay-bot (llama-3-8b)
```

## Features

### 🧠 Persistent State
Unlike standard `curl` requests where you must manage history yourself, `hugind chat` automatically maintains a local session file.
*   **Auto-Save:** Every message exchange is appended to the local history.
*   **Hibernation:** When you close the chat (Ctrl+C), the server "hibernates" your slot. The next time you run `resume`, the server reloads your context instantly.

### ⚡️ Slash Commands
Inside the chat loop, you can use special commands:
*   `/exit` or `/quit`: Saves and closes the session.
*   More commands are in development (e.g., `/clear`, `/system`).

### 🛡️ Crash Recovery
If the server crashes or restarts, your client history is safe on disk. When you reconnect, the client re-sends the necessary history to the server to restore the state transparently.
