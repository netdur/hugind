# HTTP Server Endpoints

This document describes the HTTP API served by the Hugind server.

## Base URL

The server listens on the configured `host:port`. The API base is `/v1`.

## Authentication

If an API key is configured, all endpoints require:

```
Authorization: Bearer <api_key>
```

Requests without a valid header receive `401 Unauthorized`.

## Endpoints

### `POST /v1/chat/completions`

OpenAI-compatible chat completions endpoint. Supports streaming via SSE.

Notes:

- `messages` must be non-empty.
- `stream: true` returns `text/event-stream` with `data: ...` chunks and a final `data: [DONE]`.
- `response_format: { "type": "json_object" }` enables a JSON grammar.
- Optional headers for session control:
  - `x-session-id`
  - `x-parent-id`
  - `x-request-id` (used if `x-session-id` is missing)

### `GET /v1/models`

Returns a static list of available models (current implementation returns a
single model entry).

### `GET /v1/monitor`

Returns server health and runtime statistics, including:

- server state
- request counts
- token rates
- slot usage
- memory and cache stats

### `POST /v1/embeddings`

Generates embeddings for a string or array of strings.

Notes:

- `input` can be a string or string array.
- Empty input returns `400`.

### `POST /v1/state/save`

Requests that the server save a KV cache state for a session.

### `POST /v1/state/idle`

Requests that the server idle (evict) a session.

### `DELETE /v1/state/:id`

Requests deletion of a session state by id.

## Response Codes (Common)

1. `200 OK` for successful reads.
2. `202 Accepted` for asynchronous state operations.
3. `400 Bad Request` for invalid inputs.
4. `401 Unauthorized` when the API key is missing/invalid.
5. `404 Not Found` for unknown session ids.
6. `500 Internal Server Error` for runtime failures.
