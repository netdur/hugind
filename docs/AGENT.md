# Agent User Guide

Run autonomous AI agents safely on your machine using Hugind. This guide covers how to install, run, and manage agents.

## 1. Installing Agents

To install an agent, you can use a local directory or a GitHub URL.

### Local Installation
```bash
hugind agent install ./path/to/agent-folder
```

### Remote Installation (GitHub)
You can install directly from a GitHub repository. The URL must point to the tree where `agent.yaml` is located.

```bash
hugind agent install https://github.com/user/repo/tree/main/agent-folder
```

### Security Check (Permissions)
When you install an agent, Hugind scans its manifest and asks you to approve the permissions it requires. **Read this carefully.**

- **Filesystem**: "Read/Write" access.
    - *Allowed paths*: The specific folders the agent can touch. By default, agents are sandboxed and cannot access your files unless explicitly allowed here.
- **Network**: "Allow" means it can access the internet.
    - *Allowed domains*: The specific websites it can connect to (e.g., `google.com`).
- **Shell**: "Allow" means it can run terminal commands.
    - *Whitelist*: Only the specific commands listed can be run (safe).
    - *All commands*: (Risky) The agent can run any command effectively as you.

**If an agent asks for suspicious permissions (like full shell access or access to your SSH keys), do not install it.**

## 2. Running Agents

Once installed, you can run an agent by its name:

```bash
hugind agent run agent-name
```

You can also run a local agent directly without installing (useful for testing):

```bash
hugind agent run ./my-agent-folder
```

### Passing Arguments
Any arguments you pass after the agent name are sent to the agent.
If you pass a file or folder path, **that path is automatically whitelisted** so the agent can read/write to it.

```bash
# The agent gets access to ./whitepaper.pdf
hugind agent run summarizer ./whitepaper.pdf
```

## 3. Managing Agents

List all installed agents:
```bash
hugind agent list
```

To remove an agent (currently), simply delete its folder in your config directory:
- **macOS/Linux**: `~/.hugind/agents/`
- **Windows**: `%USERPROFILE%\.hugind\agents\`

## 4. Troubleshooting

### "Connection Refused" / Server Error
Agents typically need the Hugind model server to be running to "think".
If an agent fails with a connection error, make sure you have started the server:

```bash
hugind server start metal_unified
```

(The default server config is `metal_unified`, but some agents may require specific models. Check the agent's documentation.)

### "Permission Denied"
If an agent tries to access a file or website not listed in its permissions, it will be blocked, and you will see a security error in the output. This is a safety feature.
- To fix: You (or the developer) must add the required permission to `agent.yaml` and re-install.

### Developer Info
Are you building an agent? Check the [Developer Guide](agent_dev.md) for API details and manifest specifications.
