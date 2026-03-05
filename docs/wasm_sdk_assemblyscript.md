# WASM SDK (AssemblyScript)

This document describes the AssemblyScript SDK used to call Hugind WASM
hostcalls. It is provided at `agent/cli/wasm_sdk.ts`.

This SDK is **AssemblyScript-only**. Other WASM languages need their own
bindings.

## API

### `print(msg: string): void`

Prints a message to stdout.

### `printRaw(msg: string): void`

Prints a message to stdout without appending a newline.

### `input(prompt: string): string`

Writes a prompt and reads a line from stdin.

### `netFetch(url: string): string`

Performs an HTTP GET request via the WASM hostcall and returns the response
body as text.

### `llmChat(promptOrRequestJson: string): string`

Calls the configured `/chat/completions` endpoint and returns the assistant
response text. For plain string prompts, the runtime defaults
`response_format` to `{ "type": "json_object" }`. For object request bodies,
it does not inject `response_format`.

SDK aliases:
- `llmChatRequest(requestJson: string): string`

### `llmChatStream(promptOrRequestJson: string): string`

Calls the configured `/chat/completions` endpoint with streaming enabled and
returns the full response text. For plain string prompts, the runtime defaults
`response_format` to `{ "type": "json_object" }`. For object request bodies,
it does not inject `response_format`.

If your module exports `llm_on_token(ptr: i32, len: i32)`, the runtime will
invoke it for each streamed delta, letting the agent decide whether to print.

If your module exports `llm_on_sse(ptr: i32, len: i32)`, the runtime receives
raw SSE lines as they are processed.

SDK aliases:
- `llmChatStreamRequest(requestJson: string): string`

### `runCommand(cmd: string): string`

Executes a shell command via `hugind.run_command` and returns the output.

### `toolsList(): string`

Returns a JSON string containing MCP tool descriptors
(`name`, `description`, `input_schema`, `server`).

### `toolsCall(requestJson: string): string`

Calls an MCP tool via `hugind.tools_call`.

`requestJson` must look like:

```json
{"name":"server:tool","args":{}}
```

The return value is the tool result JSON string.

### `getArgsJson(): string`

Returns the initial input JSON string. Shape includes:
- `args`
- `meta.session`
- `meta.env`

### `setResultJson(json: string): void`

Sets the agent output. The string must be valid JSON.

## Memory Helpers

### `alloc(len: i32): i32`

Grows linear memory and returns a pointer to the newly allocated region. The
SDK uses `String.UTF8.encode` and `String.UTF8.decodeUnsafe` to translate
between strings and raw bytes for hostcall interop.

## Filesystem Helpers

These functions wrap `hugind_fs` hostcalls. They are gated by
`permissions.filesystem` and `runtime_fs_mode`:

1. `fsCwd(): string`
2. `fsExists(path: string): bool`
3. `fsIsFile(path: string): bool`
4. `fsIsDir(path: string): bool`
5. `fsRealpath(path: string): string`
6. `fsReadText(path: string): string`
7. `fsReadBytes(path: string): Uint8Array`
8. `fsWriteText(path: string, data: string): void`
9. `fsWriteBytes(path: string, data: Uint8Array): void`
10. `fsAppendText(path: string, data: string): void`
11. `fsListDir(path: string): string` (JSON array of entry names)
12. `fsMkdir(path: string, recursive?: bool): void`
13. `fsRemove(path: string, recursive?: bool): void`
14. `fsRename(src: string, dst: string): void`
15. `fsCopy(src: string, dst: string): void`
16. `fsStat(path: string): string` (JSON stat object)
