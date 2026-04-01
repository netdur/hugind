# Agent JavaScript Runtime

This document describes how Hugind executes JavaScript agents and what the JS
runtime exposes to agent code.

## When JS Runtime Is Used

An agent runs in the JavaScript runtime when its `entry_point` is a `.js` file.
If the entry point is a `.wasm` file, the WASM runtime is used instead.

## Module Loading Rules

JavaScript modules are loaded with a local-only resolver:

1. Only relative imports are allowed (`./` or `../`).
2. Imports must resolve inside the agent root directory.
3. Only `.js` modules are allowed.

Imports that violate these rules raise a runtime error.

## Entry Module Contract

The entry module is evaluated, then Hugind looks for a default export:

1. If a default export function exists, it is called with a single argument
   containing initial data.
2. If the default export returns a Promise, Hugind waits for it to resolve.
3. If no default export exists, the result is `null`.

The returned value is converted to JSON and used as the agent's output.

## Passing Input

Hugind provides the initial arguments in two ways:

1. Default export parameter (preferred):
   - `export default async function (input) { ... }`
2. Global helper:
   - `get_args_json()` returns the JSON string for the initial input.
   - `get_args()` is an alias for `get_args_json()`.

Input shape:
- `input.args`: CLI args passed after `--`.
- `input.meta.session`: resolved backend session metadata.
- `input.meta.env`: environment values declared in `agent.yaml` `env:` section.
  Only declared env names are passed through, and required vars fail the run
  if missing from the host environment.

## Returning Output

Agents can return output in two ways:

1. Return a value from the default export (or a resolved Promise).
2. Call `set_result(value)` to explicitly set the output.

If both are used, `set_result` takes precedence once it is observed.

## Global Functions and Capabilities

These globals are installed at runtime:

### `print(message: string)`

Writes to stdout.

### `print_raw(message: string)`

Writes to stdout without appending a newline.

### `hugind_version() -> string`

Returns the current Hugind runtime version.

### `input(prompt: string) -> Promise<string>`

Writes a prompt, then reads a line from stdin.

### `net.fetch(url: string) -> Promise<string>`

Performs an HTTP GET request and returns the response body as text.
Network access is gated by the agent's `permissions.network`:

- `allow` must be `true`.
- If `allowed_domains` or `allowed_ips` is non-empty, the host must match.
- If `block_private_networks` is true, private/loopback IPs are blocked.
- `timeout` and `max_response_bytes` are enforced.
- Redirects are followed up to 5 times.

### `llm.chat(prompt: string) -> Promise<string>`

Sends a chat completion request to the configured backend and returns the
assistant response text. The backend is resolved from the agent manifest.
For plain string prompts, `response_format` defaults to
`{ "type": "json_object" }`. For object request bodies, no `response_format`
is injected.

### `llm.chat_stream(prompt: string | object) -> Promise<string>`

Calls the chat completion endpoint with streaming enabled and returns the
full response text. For plain string prompts, `response_format` defaults to
`{ "type": "json_object" }`. For object request bodies, no `response_format`
is injected.

If the input is an object, you may provide `on_token` (or `onToken`) callback
function. It will be called for each streamed delta, letting the agent decide
whether to print.

### `run_command(cmd: string) -> Promise<string>`

Executes a shell command using `permissions.shell`:

- `allow` must be `true`.
- `whitelist` is enforced if present.
- `blacklist` blocks if present.
- `timeout`, `max_output`, `env_clear`, and `working_dir` are applied.

`runCommand(cmd)` is an alias for `run_command(cmd)`.

### `spawn(program: string, args: string[]) -> Promise<string>`

Executes a command directly without a shell, bypassing shell escaping issues.
Arguments are passed exactly as provided.

- `allow` must be `true`.
- `whitelist` is enforced if present.
- `blacklist` blocks if present.
- `timeout`, `max_output`, `env_clear`, and `working_dir` are applied.

### `tools.list() -> Promise<string>`

Returns a JSON string for an array of tool descriptors
(`name`, `description`, `input_schema`, `server`).

### `tools.call(name: string, args: object) -> Promise<string>`

Calls an MCP tool by name and returns the tool result as a JSON string.

Tool naming:
- When multiple MCP servers are configured, use `server:tool`.
- With a single MCP server, unqualified `tool` is accepted.

### `fs.*` (host filesystem API)

Host filesystem calls are gated by `permissions.filesystem` and
`runtime_fs_mode` (same semantics as the WASM runtime).

Available methods:

1. `fs.cwd() -> string`
2. `fs.exists(path: string) -> bool`
3. `fs.is_file(path: string) -> bool`
4. `fs.is_dir(path: string) -> bool`
5. `fs.realpath(path: string) -> string`
6. `fs.read_text(path: string) -> string`
7. `fs.read_bytes(path: string) -> number[]` (0..255)
8. `fs.write_text(path: string, data: string) -> void`
9. `fs.write_bytes(path: string, data: string | number[]) -> void`
10. `fs.append_text(path: string, data: string) -> void`
11. `fs.list_dir(path: string) -> string` (JSON array of entry names)
12. `fs.mkdir(path: string, recursive: bool) -> void`
13. `fs.remove(path: string, recursive: bool) -> void`
14. `fs.rename(src: string, dst: string) -> void`
15. `fs.copy(src: string, dst: string) -> void`
16. `fs.stat(path: string) -> string` (JSON stat object)

## Agentic Mode Globals

When `mode: agentic` is set in `agent.yaml`, these additional globals are available:

### `register_tool(def)` / `registerTool(def)`

Registers a tool for the agentic loop. The LLM can invoke registered tools
via `<tool_call>` tags in its response.

```js
register_tool({
  name: "read_file",
  description: "Read a file's contents",
  parameters: {
    type: "object",
    properties: { path: { type: "string" } },
    required: ["path"]
  },
  execute: (args_json) => {
    var args = JSON.parse(args_json);
    return fs.read_text(args.path);
  }
});
```

The `execute` function receives a JSON string of arguments and should return
a string result. Async functions (returning Promises) are supported — the
runtime awaits them automatically.

### `set_system_prompt(prompt)` / `setSystemPrompt(prompt)`

Sets the system prompt for the agentic loop. Called during agent setup.

### `set_max_turns(n)` / `setMaxTurns(n)`

Overrides the maximum number of LLM round-trips (default: 10, or from YAML).

## Team Context Globals

When running in a team (workflow or `hugind agent team`), these globals are
available for inter-agent communication:

### `memory.set(key, value)`

Write a value to shared memory under the agent's namespace.

### `memory.get(key) -> string`

Read a value by fully-qualified key (e.g. `"architect/spec"`). Returns JSON string or `"null"`.

### `memory.list() -> string`

Returns all shared memory entries as a JSON object.

### `memory.summary() -> string`

Returns a markdown summary grouped by agent.

### `messaging.send(to, content)`

Send a point-to-point message to another agent.

### `messaging.broadcast(content)`

Broadcast a message to all team members.

### `messaging.receive() -> string`

Returns unread messages as a JSON array of `{from, to, content}` objects.

### `tasks.spawn(json) -> string`

Dynamically spawn a new task (only available in workflow execution with task queue):

```js
tasks.spawn(JSON.stringify({
  title: "Fix auth bug",
  description: "Auth returns 500",
  assignee: "developer",
  depends_on: []
}));
```

## Error Handling

If the JS runtime throws an exception, Hugind prints the exception and stack
trace (when available) and the agent run fails.

## Limits and Notes

1. JS execution is sandboxed by module resolution rules, but JS code can still
   access the provided globals. Capabilities are intentionally narrow.
2. Only `.js` modules are supported; other extensions are rejected.
3. Networking supports only GET via `net.fetch`.
