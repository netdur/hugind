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
   containing initial data (currently `{ args: [...] }`).
2. If the default export returns a Promise, Hugind waits for it to resolve.
3. If no default export exists, the result is `null`.

The returned value is converted to JSON and used as the agent's output.

## Passing Input

Hugind provides the initial arguments in two ways:

1. Default export parameter (preferred):
   - `export default async function (input) { ... }`
2. Global helper:
   - `get_args_json()` returns the JSON string for the initial input.

## Returning Output

Agents can return output in two ways:

1. Return a value from the default export (or a resolved Promise).
2. Call `set_result(value)` to explicitly set the output.

If both are used, `set_result` takes precedence once it is observed.

## Global Functions and Capabilities

These globals are installed at runtime:

### `print(message: string)`

Writes to stdout.

### `input(prompt: string) -> Promise<string>`

Writes a prompt, then reads a line from stdin.

### `net.fetch(url: string) -> Promise<string>`

Performs an HTTP GET request and returns the response body as text.
Network access is gated by the agent's `permissions.network`:

- `allow` must be `true`.
- If `allowed_domains` is non-empty, the host must match one of them.

### `llm.chat(prompt: string) -> Promise<string>`

Sends a chat completion request to the configured backend and returns the
assistant response text. The backend is resolved from the agent manifest.

## Error Handling

If the JS runtime throws an exception, Hugind prints the exception and stack
trace (when available) and the agent run fails.

## Limits and Notes

1. JS execution is sandboxed by module resolution rules, but JS code can still
   access the provided globals. Capabilities are intentionally narrow.
2. Only `.js` modules are supported; other extensions are rejected.
3. Networking supports only GET via `net.fetch`.
