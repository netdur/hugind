# Agent Runtime

Agents are sandboxed Dart scripts executed by Hugind with explicit, manifest-based permissions. See `docs/cli.md` for how `<config_home>` is resolved.

## Commands

### `hugind agent install <path>`
Installs an agent from a local directory containing `agent.yaml`. (Remote URLs are not supported yet.)

Behavior:
- Validates `agent.yaml` and agent name.
- Shows requested permissions (network, filesystem, MCP).
- Prompts for confirmation before installation.
- Installs to `<config_home>/agents/<agent_name>/`.

### `hugind agent list`
Lists installed agents with version and description (if present in `agent.yaml`).

### `hugind agent run <agent_name> [args...]`
Runs an installed agent.

Notes:
- You can also run a local agent by path: `hugind agent run ./my-agent`.
- The agent connects to a server config (default `metal_unified`) unless overridden in `agent.yaml`.
- If the first argument looks like a directory, it is added to the allowed paths for `workDir`.

## Manifest (`agent.yaml`)

Required fields:

- `name` (alphanumeric, dash, underscore)
- `entry_point` (defaults to `main.dart`)
- `backend` (config name; defaults to `metal_unified`)

Permissions are optional but recommended:

```yaml
name: "example-agent"
version: "1.0.0"
description: "Demo agent"
entry_point: "main.dart"
backend: "metal_unified"

permissions:
  filesystem:
    allowed_paths:
      - "/Users/me/projects"
  network:
    allowed_domains:
      - "api.example.com"
  shell:
    allow: false

dependencies:
  mcp:
    - name: "filesystem"
```

## Capabilities Available to Agents

At runtime, the agent can request access to capabilities based on the manifest:

- `sys.run(...)` executes shell commands (only if `permissions.shell.allow` is true)
- `sys.confirm(...)` and `sys.readInput(...)` for user interaction
- `llm.chat(...)` calls the local inference server
- `net.fetch(...)` makes HTTP requests to allowed domains
- MCP tools are exposed via `sys.tools.list()` and `sys.tools.call(...)`

## Best Practices

- The agent directory is always allowed for filesystem access; add `allowed_paths` for extra roots.
- Use least privilege: only allow the exact domains and paths required.
- Keep agent code self-contained within its directory.
- Prefer MCP tools for filesystem/database access instead of shelling out.
- Verify the server is running before calling `hugind agent run`.
