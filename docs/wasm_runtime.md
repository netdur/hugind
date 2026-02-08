# Agent WASM Runtime

This document describes how Hugind executes WebAssembly agents and what the
WASM hostcalls expose to guest code.

## When WASM Runtime Is Used

An agent runs in the WASM runtime when its `entry_point` is a `.wasm` file.
If the entry point is a `.js` file, the JavaScript runtime is used instead.

## Execution Model

1. The module is instantiated with WASI preview1 support.
2. Hugind calls `_start` if present; otherwise it calls `main`.
3. The agent can read input via `hugind.get_args` and return output via
   `hugind.set_result`.

The final result is the JSON value passed to `hugind.set_result`, or `null` if
none is set.

## Filesystem Modes

The WASM runtime has two filesystem access mechanisms:

1. **WASI mounts** (sandbox filesystem)
   - Only explicitly mounted host directories are visible.
   - Mounts grant full access within the mounted directories.
2. **Host filesystem API** (native hostcalls)
   - Access is gated by `permissions.filesystem`.

`runtime_fs_mode` controls which mechanism is enabled. In the current runtime:

- `host_filesystem` disables WASI mounts.
- Any other value leaves WASI mounts enabled.

Host filesystem calls are always gated by `permissions.filesystem`. WASI mounts
are independent of those permissions.

## Resource Limits

WASM resource limits come from `wasm.resources`:

- `memory`: sets a store memory limit if provided.
- `cpu`: enables fuel tracking (fuel is currently set to a fixed budget).
- `timeout`: overall execution timeout.
- `max_output`: currently unused by the runtime.

## Hostcalls

Hostcalls are exposed under `hugind` and `hugind_fs`. All string data is passed
as UTF‑8 bytes with pointer/length pairs.

### `hugind.print(message)`

Prints a message to stdout.

### `hugind.input(prompt) -> string`

Writes a prompt and reads a line from stdin.

### `hugind.get_args() -> string`

Returns the initial input JSON string (currently `{ "args": [...] }`).

### `hugind.set_result(json_string)`

Sets the result JSON for the agent run. The string must be valid JSON.

### `hugind.net_fetch(url) -> string`

Performs an HTTP GET request with permission checks:

- `permissions.network.allow` must be `true`.
- If `allowed_domains` or `allowed_ips` is non‑empty, the host must match.
- If `block_private_networks` is true, private/loopback IPs are blocked.
- `timeout` and `max_response_bytes` are enforced.

Redirects are followed up to 5 times.

### `hugind.llm_chat(prompt) -> string`

Calls the configured `/chat/completions` endpoint (non‑streaming) and returns
the assistant content. The response is capped at 10 MB.

### `hugind.llm_chat_stream(prompt) -> string`

Calls the configured `/chat/completions` endpoint with streaming enabled,
prints streamed deltas to stdout, and returns the full content (capped at 10 MB).

### `hugind.run_command(command) -> string`

Executes a shell command using `permissions.shell`:

- `allow` must be `true`.
- `whitelist` is enforced if present.
- `blacklist` blocks if present.
- `timeout`, `max_output`, `env_clear`, and `working_dir` are applied.

On macOS, commands are run via `sandbox-exec` with a minimal profile.

### `hugind_fs.*`

Host filesystem calls (gated by `permissions.filesystem` and `runtime_fs_mode`):

1. `hugind_fs.fs_cwd() -> string`
2. `hugind_fs.fs_exists(path) -> int`
3. `hugind_fs.fs_is_file(path) -> int`
4. `hugind_fs.fs_is_dir(path) -> int`
5. `hugind_fs.fs_realpath(path) -> string`
6. `hugind_fs.fs_read_text(path) -> string`
7. `hugind_fs.fs_read_bytes(path) -> bytes`
8. `hugind_fs.fs_write_text(path, data)`
9. `hugind_fs.fs_write_bytes(path, data)`
10. `hugind_fs.fs_append_text(path, data)`
11. `hugind_fs.fs_list_dir(path) -> string (JSON)`
12. `hugind_fs.fs_mkdir(path, recursive)`
13. `hugind_fs.fs_remove(path, recursive)`
14. `hugind_fs.fs_rename(src, dst)`
15. `hugind_fs.fs_copy(src, dst)`
16. `hugind_fs.fs_stat(path) -> string (JSON)`

Return values are encoded as strings or JSON where noted.

## Errors

WASM traps or hostcall errors cause the agent run to fail. `abort()` is
intercepted and reported as a guest execution error.

## WASM SDK

See `docs/wasm_sdk_assemblyscript.md` for the AssemblyScript SDK and usage.
