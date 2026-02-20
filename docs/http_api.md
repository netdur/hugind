# HTTP Server Endpoints

This document describes the HTTP API served by the Hugind server implementation in `src/server`.

## Base URL

The server listens on configured `host:port`. API routes are under `/v1`.

## Authentication

If an API key is configured, all endpoints require:

```http
Authorization: Bearer <api_key>
```

If no API key is configured, requests are unauthenticated.

## Endpoints

### `POST /v1/chat/completions`

OpenAI-style chat endpoint with optional SSE streaming.

Request body fields currently handled:

- `model` (string): echoed back in responses.
- `messages` (array, required, non-empty).
- `stream` (bool, optional): defaults to `false`.
- `max_tokens` (u32, optional): defaults to `1024`.
- `temperature` (f32, optional): defaults to `0.8`.
- `top_p` (f32, optional): defaults to `0.9`.
- `frequency_penalty` (f32, optional): mapped to repeat penalty as `1.0 + max(0, frequency_penalty)`.
- `response_format` (optional): `{ "type": "json_object" }` enables JSON grammar-constrained decoding.

Accepted but currently ignored:

- `presence_penalty`
- `stop`

`messages[].content` supports both:

- plain text string
- multimodal array with parts:
  - `{ "type": "text", "text": "..." }`
  - `{ "type": "image_url", "image_url": { "url": "..." } }`

Image URL constraints:

- Supports `data:*;base64,...` URLs.
- Supports `http://` and `https://` URLs (fetched server-side).
- Other schemes return `400`.

Optional headers for session control:

- `x-session-id`
- `x-request-id` (used only if `x-session-id` is absent)
- `x-parent-id`

Streaming behavior (`stream: true`):

- Response type: `text/event-stream`.
- Emits `data: <json chunk>` events.
- Emits a final chunk containing `finish_reason`, then `data: [DONE]`.
- Engine failures are emitted as SSE `event: error`.

Non-stream response is a single `chat.completion` JSON payload (`usage` is currently `null`).

### `GET /v1/models`

Returns one model entry:

- `id` = `config_name`, else runtime `model_name`, else `"unknown"`.

### `GET /v1/monitor`

Returns:

- `config_name`
- `server_state`
- `requests_processing`
- `requests_waiting`
- `tokens_per_sec_total`
- `tokens_per_sec_per_active`
- `slots_usage` (`active`, `total`)
- `memory` (`ram_usage_bytes`, `vram_usage_bytes`)
- `cache_stats` (`vram_sessions`, `ram_sessions`)

Note: `vram_usage_bytes` is currently always `null`.

### `POST /v1/embeddings`

Generates embeddings for one or many strings.

Request fields:

- `model` (string): echoed back in response.
- `input` (string or string array, required).
- `encoding_format` (optional, currently ignored).

Notes:

- Empty `input` array returns `400`.
- Response object is `"list"` with `data[]` entries containing float vectors.
- `usage.prompt_tokens` and `usage.total_tokens` are currently `0`.

### `POST /v1/state/save`

Queues KV-cache save for an active session.

Body:

- `session_id` (required): active session id.
- `template_id` (required): filename key for output.

Behavior:

- Writes to `<data_home>/sessions/<template_id>.bin` (`~/.hugind/sessions/...` on typical Unix setups).
- Returns `202` when queued.

### `POST /v1/state/idle`

Queues idle/evict action for an active session.

Body:

- `session_id` (required)

### `GET /v1/state/:id`

Checks whether a session state is currently available in any backing tier.

Availability is considered `true` when at least one of these is true:

- VRAM-backed state is bound to a live sequence for that session.
- RAM snapshot exists for that session.
- Disk state file exists at the session's tracked disk path.

Response (`200` when present):

```json
{
  "session_id": "<id>",
  "exists": true
}
```

Response (`404` when missing):

```json
{
  "session_id": "<id>",
  "exists": false
}
```

Client recovery pattern:

- Call this endpoint before continuing a prior chat session.
- If `404`/`exists: false`, resend full chat history on the next completion request.

### `DELETE /v1/state/:id`

Queues deletion for an active session id.

- `:id` is the session id.

## Response Codes (Common)

1. `200 OK` successful synchronous requests.
2. `202 Accepted` state mutation requests queued (`state/save`, `state/idle`, `state/:id` delete).
3. `400 Bad Request` invalid request data (for example empty `messages`, invalid image URL/data URL, empty embedding input).
4. `401 Unauthorized` missing/invalid bearer token when API key auth is enabled.
5. `404 Not Found` unknown session id on state endpoints.
6. `500 Internal Server Error` runtime/engine failures.
