# Hugind Stdio Bridge

The stdio bridge exposes Hugind operations over newline-delimited JSON (NDJSON)
and also supports MCP JSON-RPC 2.0.

## Launch

```bash
hugind stdio
```

Each input line must be one JSON object. Each output line is one JSON object.

## NDJSON Protocol

### Request envelope

```json
{"id":"req-1","method":"model.list","params":{}}
```

### Success response

```json
{"id":"req-1","ok":true,"result":{...},"schema_version":"1"}
```

### Error response

```json
{"id":"req-1","ok":false,"error":{"code":"internal_error","message":"..."},"schema_version":"1"}
```

### Event envelope

```json
{"event":"status","id":"req-1","data":{...},"schema_version":"1"}
```

Event types currently emitted:

1. `status`
2. `progress`
3. `log`
4. `agent_event` (agentic loop events — see below)

## Supported Methods

### Agent

1. `agent.list` (no params)
2. `agent.run`
   params: `{ "path": string, "args"?: string[] }`
   events: `status` (`agent.run.start`/`agent.run.finish`), `log`, `agent_event`
3. `agent.install`
   params: `{ "path": string, "approve_permissions": bool, "overwrite": bool }`
4. `agent.remove`
   params: `{ "name": string }`

### Config

1. `config.list` (no params)
2. `config.validate`
   params: `{ "path": string }`
3. `config.info` (no params)
4. `config.remove`
   params: `{ "name": string, "confirm": bool }`
5. `config.defaults`
   params: `{ "hf_token"?: string }`
6. `config.init`
   params:
   `{ "name": string, "model_path": string, "ctx"?: number, "mmproj_path"?: string, "overwrite"?: bool, "n_slots"?: number, "enable_fit"?: bool, "fit_target_mib"?: number[] }`

### Model

1. `model.list` (no params)
2. `model.show`
   params: `{ "repo": string }`
3. `model.add`
   params: `{ "repo": string, "files": string[] }`
   events: `status`, `progress`
4. `model.remove`
   params:
   `{ "repo": string, "files"?: string[], "delete_repo"?: bool, "delete_if_empty"?: bool }`

### Server

1. `server.list` (no params)
2. `server.start`
   params: `{ "config": string, "port"?: number }`
   events: `status` (`data.status: "started"` when server port is reachable)
3. `server.stop`
   params: `{ "config": string }`
   events: `status` (`data.status: "stopped" | "already_stopped" | "stop_timeout"`)

## Result Shapes (high-level)

1. `agent.list` -> `{ agents: [{ name, path }] }`
2. `config.list` -> `{ configs: [{ name, path }] }`
3. `model.list` -> `{ repos: [{ name, path }] }`
4. `server.list` -> `{ servers: [{ name, host, port, status, config_name? }] }`

Other methods return operation-specific objects (for example `removed`, `valid`,
`deleted_files`, `status`).

## Agent Events

When running an agentic agent via `agent.run`, the stdio bridge emits structured
`agent_event` events during the agentic loop. These allow a UI to show real-time
progress.

### Event envelope

```json
{"event":"agent_event","id":"req-1","data":{"type":"agent.turn",...},"schema_version":"1"}
```

The `data.type` field identifies the event kind.

### Event types

| type | data fields | description |
|---|---|---|
| `agent.setup` | `tool_count` | Emitted after tools are registered and the loop begins |
| `agent.turn` | `turn`, `max_turns`, `message_count` | Emitted at the start of each LLM turn |
| `agent.tool_call` | `name`, `args` | Emitted before a tool is executed |
| `agent.tool_result` | `name`, `result`, `duration_ms` | Emitted after a tool finishes (result truncated to 2KB) |
| `agent.progress` | `message` | Agent progress/thinking lines (from `eprint()` in JS) |
| `agent.complete` | `turns`, `final_len` | Emitted when the agent produces a final answer |

### Example event stream

```
← {"event":"status","id":"req-1","data":{"message":"agent.run.start"},...}
← {"event":"agent_event","id":"req-1","data":{"type":"agent.progress","message":"  → Scanning project structure..."},...}
← {"event":"agent_event","id":"req-1","data":{"type":"agent.setup","tool_count":5},...}
← {"event":"agent_event","id":"req-1","data":{"type":"agent.turn","turn":0,"max_turns":10,"message_count":2},...}
← {"event":"agent_event","id":"req-1","data":{"type":"agent.tool_call","name":"run","args":{"command":"ls /Applications"}},...}
← {"event":"agent_event","id":"req-1","data":{"type":"agent.progress","message":"  → Running: ls /Applications"},...}
← {"event":"agent_event","id":"req-1","data":{"type":"agent.tool_result","name":"run","result":"Android Studio.app\n...","duration_ms":18},...}
← {"event":"agent_event","id":"req-1","data":{"type":"agent.turn","turn":1,"max_turns":10,"message_count":4},...}
← {"event":"agent_event","id":"req-1","data":{"type":"agent.complete","turns":2,"final_len":307},...}
← {"event":"status","id":"req-1","data":{"message":"agent.run.finish"},...}
← {"id":"req-1","ok":true,"result":{"status":"ok","result":"## Answer\n..."},...}
```

### Notes

- `agent.progress` events carry the `eprint()` output from agent JS code (the
  `→` progress lines). In CLI mode, these go to stderr instead.
- `agent.tool_result.result` is truncated to 2KB to avoid flooding the transport.
  The full result is available in the agent log file.
- Events are only emitted for agentic agents (`mode: agentic`). Script mode
  agents only produce `status` and `log` events.

## MCP Compatibility

Messages with `"jsonrpc":"2.0"` are handled as MCP JSON-RPC.

### Implemented MCP methods

1. `initialize`
2. `notifications/initialized`
3. `tools/list`
4. `tools/call`

`tools/call` expects:

```json
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"model.list","arguments":{}}}
```

The tool result is returned as MCP `content` with stringified JSON payload.

### MCP notifications emitted by Hugind

1. `notifications/hugind.status`
2. `notifications/hugind.progress`
3. `notifications/hugind.log`
4. `notifications/hugind.agent_event` — agentic loop events (same `data.type`
   values as the NDJSON `agent_event` above)

Each includes the request id in `params.id`.
