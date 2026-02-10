# Hugind Stdio Bridge

The stdio bridge exposes Hugind CLI capabilities over newline-delimited JSON (NDJSON) on stdin/stdout. It is designed for desktop/dashboard integrations (e.g., Flutter).

It also supports MCP (JSON-RPC 2.0) over stdio for tool discovery and calls.

## Launch

```
hugind stdio
```

All requests are JSON objects, one per line. All responses/events are also JSON objects, one per line.

## Envelope (NDJSON)

Every message is one JSON object per line.

### Request

```json
{"id":"uuid","method":"model.add","params":{...}}
```

### Response

```json
{"id":"uuid","ok":true,"result":{...},"schema_version":"1"}
```

### Error

```json
{"id":"uuid","ok":false,"error":{"code":"...","message":"..."},"schema_version":"1"}
```

### Event

```json
{"event":"progress","id":"uuid","data":{...},"schema_version":"1"}
```

## Events

- `progress`: download progress for `model.add`.
- `status`: milestone messages.
- `log`: streamed output from `hugind.print`/`hugind.print_raw` during `agent.run`.

## Methods

### `agent.list`
Params: none

### `agent.run`
Params:
- `path` (string)
- `args` (array of strings, optional)

Events:
- `log` (agent output)
- `status` (start/finish)

### `agent.install`
Params:
- `path` (string)
- `approve_permissions` (bool)
- `overwrite` (bool)

### `agent.remove`
Params:
- `name` (string)

### `config.list`
Params: none

### `config.validate`
Params:
- `path` (string)

### `config.info`
Params: none

### `config.remove`
Params:
- `name` (string)
- `confirm` (bool)

### `config.defaults`
Params:
- `lib` (string, optional)
- `hf_token` (string, optional)

### `config.init`
Params:
- `name` (string)
- `model_path` (string, required)
- `preset` (string, optional: `metal_unified`, `cuda_dedicated`, `cpu_only`)
- `ctx` (number, optional)
- `mmproj_path` (string, optional)
- `format` (string, optional)
- `overwrite` (bool, optional)

### `model.list`
Params: none

### `model.show`
Params:
- `repo` (string)

### `model.add`
Params:
- `repo` (string)
- `files` (array of strings)

Events:
- `progress` (bytes downloaded)
- `status` (start/finish per file)

### `model.remove`
Params:
- `repo` (string)
- `files` (array of strings, optional)
- `delete_repo` (bool, optional)
- `delete_if_empty` (bool, optional)

### `server.list`
Params: none

### `server.start`
Params:
- `config` (string)
- `port` (number, optional)

### `server.stop`
Params:
- `config` (string)

## Example

Request:

```json
{"id":"1","method":"model.list","params":{}}
```

Response:

```json
{"id":"1","ok":true,"result":{"repos":[{"name":"google/gemma-3-4b-it-qat-q4_0-gguf","path":"/Users/..."}]},"schema_version":"1"}
```

## MCP Compatibility (JSON-RPC 2.0)

The same stdio process also accepts MCP-style JSON-RPC 2.0 messages.

### Initialize

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
```

### Tools List

```json
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
```

### Tools Call

```json
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"model.list","arguments":{}}}
```

### MCP Notifications (custom)

During long-running operations, Hugind emits JSON-RPC notifications:

- `notifications/hugind.progress`
- `notifications/hugind.status`
- `notifications/hugind.log`

Each notification includes the original request `id` in `params.id`.
