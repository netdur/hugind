# Agent Manifest

This document describes how `agent.yaml` is interpreted by the current Hugind
runtime.

## Top-Level Fields

### Runtime fields (used by code)

- `name` (string): agent identifier. Used for install/log naming, not as a
  session id.
- `version` (string): agent version.
- `hugind_version` (string, optional): semver constraint checked at runtime
  (example: `>=0.6.0`).
- `entry_point` (string): entry module path (`.js` or `.wasm`).
- `wasm` (object, optional): WASM runtime config.
- `backend` (object, optional): API base URL/model/session config.
- `permissions` (object, optional): host capability controls.
- `dependencies` (object, optional): MCP servers.
- `env` (array, optional): environment variables exposed to the agent.

### Metadata fields (accepted but currently ignored by runtime)

- `description`
- `author`
- `license`

Unknown extra fields are also tolerated.

## `wasm` Section

Used when `entry_point` points to a `.wasm` module.

### `runtime_fs_mode`

- `wasi_mounts`: only WASI mounts are available; host FS APIs are disabled.
- `host_filesystem`: only host FS APIs are available; no WASI mounts are added.
- `both` (default): enables both.

### `mounts`

List of `{ host, guest }` mappings for WASI preopens.

- `guest` must be absolute and cannot contain `..`.
- `host` is canonicalized.
- If `host` is outside agent root, it is rejected unless
  `permissions.filesystem.allow_outside_agent_root: true`.

### `resources`

- `memory`: enforced as a store memory limit when parseable (e.g. `512MB`).
- `timeout`: enforced as overall WASM execution timeout.
- `cpu`: currently only toggles fuel mode with a fixed fuel budget.
- `max_output`: currently parsed in schema but not enforced globally for WASM.

## `backend` Section

### `url`

Base API URL (default: `http://127.0.0.1:8080/v1`).

### `config`

String used as default model name for LLM hostcalls when model is omitted.

If `url` is not provided, Hugind also tries to resolve a config file from
`~/.hugind/configs/<config>.yml|yaml` and builds base URL from
`server.host`/`server.port`.

### `session`

- `mode`: `stateless` | `fresh` | `resume`
- `id`: required for `resume`; ignored for `fresh`

Semantics:
- `stateless`: no `X-Session-ID` header.
- `fresh`: generates UUID v4, sends it as `X-Session-ID`, then deletes
  `/state/:id` at run end.
- `resume`: requires non-empty `id`, sends `X-Session-ID`, no auto-delete.

Note: if `backend` object is present, it must contain at least `url` or
`config`.

## `permissions` Section

Applies to host capabilities (`net`, host FS APIs, shell/process).

### `network`

- `allow`: master switch.
- `allowed_domains`: domain allowlist (`example.com` also matches subdomains).
- `allowed_ips`: exact IP allowlist entries (not CIDR parsing).
- `block_private_networks`: blocks resolved private/loopback/link-local targets.
- `timeout`, `max_response_bytes`: apply to host network fetch calls.

### `filesystem`

Controls host FS APIs only (not WASI mounts).

- `allow`: master switch.
- `read`, `write`, `create`, `delete`: operation flags.
- `allow_outside_agent_root`: allows scopes outside agent root.
- `allowed_paths`, `denied_paths`: prefix-based path policy.
- `follow_symlinks`: whether to resolve symlinks during checks.

Behavior notes:
- If `allowed_paths` is empty and `allow_outside_agent_root` is `false`, access
  is scoped to runtime FS root.
- `hugind agent run --cwd <path>` changes runtime FS root; outside-agent paths
  require `allow_outside_agent_root: true`.
- `hugind agent run --log-file <path>` only affects runtime log destination.

### `shell`

- `allow`: master switch.
- `whitelist`: only listed commands allowed (exact program match).
- `blacklist`: listed commands denied.
- `timeout`, `max_output`, `env_clear`, `working_dir`: execution guards.

If both whitelist and blacklist are set, both checks apply.

## `dependencies` Section

MCP servers are configured under `dependencies.mcp`.

Supported keys per server:
- `name` (required)
- `required` (default `false`)
- `transport` (only `stdio` is supported)
- `command` (required if `required: true`)
- `args`
- `env`
- `cwd`
- `version` and `description` (metadata)

Runtime behavior:
- Missing command on `required: true` fails startup.
- Missing command on optional entries skips that server.
- Tool names are `server:tool`; with one server, bare `tool` is accepted.

JavaScript runtime:
- Global `tools.list()` and `tools.call(name, args)` async APIs.
- Both return JSON strings.

WASM runtime:
- Host imports `hugind.tools_list` and `hugind.tools_call`.
- `tools_call` input JSON format: `{"name":"server:tool","args":{...}}`.

## `env` Section

Each item can be:
- a string: `"VAR_NAME"` (optional variable)
- an object: `{ name: "VAR_NAME", required: true|false, ... }`

Runtime behavior:
- Values are read from host environment at run time.
- Injected under `input.meta.env`.
- Missing required vars fail the run.
- Extra keys (for example `description`) are accepted but not interpreted.

## Reference Template

See `assets/agent.yaml` for a full annotated example.
