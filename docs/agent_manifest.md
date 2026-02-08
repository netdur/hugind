# Agent Manifest

This document describes the `agent.yaml` manifest format used by Hugind. It
explains each field, its intent, and how it is interpreted by the runtime.
When you run `hugind agent run <agent-name>`, Hugind reads the agent's
`agent.yaml` to determine how to execute it.

## Top-Level Fields

### `name`

Human-readable identifier for the agent. Used as a default session id if one is
not provided.

### `version`

Agent package version. Use semantic versioning.

### `description`

Short description of what the agent does.

### `author`

Package author or maintainer.

### `license`

License identifier (e.g. `MIT`).

### `hugind_version`

Compatibility constraint for the Hugind runtime that can execute this agent.
Example: `>=0.6.0`.

### `entry_point`

Main executable artifact. Supported forms:

- JavaScript file: `src/index.js`
- WebAssembly module: `dist/agent.wasm`

## `wasm` Section

Configuration for the WebAssembly runtime. This applies when `entry_point` is a
WASM module.

### `runtime_fs_mode`

Defines filesystem access mode for the guest:

- `wasi_mounts`: guest sees only explicit mounts.
- `host_filesystem`: guest uses host-provided FS APIs, gated by permissions.
- `both`: enable both (default).

If both are enabled, mounts provide workspace-like access while host FS access
remains restricted by permissions.

### `mounts`

Maps host paths to guest paths. Each mount grants full access inside the mapped
directory.

Example:

```yaml
mounts:
  - host: "./data"
    guest: "/data"
```

Implementations should canonicalize paths to prevent symlink escapes.

### `resources`

Resource limits for the WASM instance:

- `memory`: max memory (e.g. `512MB`, `1GB`).
- `cpu`: CPU budget (percentage or fuel units).
- `timeout`: optional wall-clock timeout (e.g. `60s`).
- `max_output`: optional stdout/stderr cap (e.g. `1MB`).

## `backend` Section

Defines the HTTP API connection used by the agent.

### `url`

Base URL for the Hugind server API. Example: `http://127.0.0.1:8080/v1`.

### `config`

Name of the server config to use by default. This should match a config in
`~/.hugind/configs`.

### `session`

Session behavior:

- `mode`: `stateless` | `fresh` | `resume`.
- `id`: optional session id. Used only for `resume`. Ignored for `fresh`.

Runtime semantics:
- `stateless`: no `X-Session-ID` header is sent.
- `fresh`: runtime generates a UUID4 at the start of the run, uses it as
  `X-Session-ID` for all requests, then calls `DELETE /v1/state/:id` after the
  run completes.
- `resume`: runtime requires `id` and sends it as `X-Session-ID`; no auto-delete.

## `permissions` Section

Defines the security boundary for host-provided capabilities.

### `network`

Controls host networking functions.

- `allow`: master switch.
- `allowed_domains`: list of permitted domains.
- `allowed_ips`: optional IP ranges (prefer domains).
- `block_private_networks`: optional block on private/loopback ranges.

### `filesystem`

Controls host filesystem APIs. This does not restrict WASI mounts.

- `allow`: master switch for host FS access.
- `read`, `write`, `create`, `delete`: fine-grained operations.
- `allowed_paths`: list of allowed path prefixes.
- `denied_paths`: optional deny list (if supported).
- `follow_symlinks`: optional, recommended `false`.

### `shell`

Controls process execution.

- `allow`: master switch.
- `whitelist`: strict allow list (recommended).
- `blacklist`: deny list (do not use with `whitelist`).
- `timeout`, `max_output`, `env_clear`, `working_dir`: optional execution guards.

## `dependencies` Section

Defines external Model Context Protocol (MCP) tools required by the agent.

Each dependency includes:

- `name`
- `version`
- `required`
- `description`

## `env` Section

Environment variables required by the agent. Each entry includes:

- `name`
- `description`
- `required`

## Example

Refer to the reference template for a complete annotated file.
