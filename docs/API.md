# API Reference

Hugind exposes an OpenAI-compatible HTTP API.

Base URL: `http://127.0.0.1:<port>/v1`

## Endpoints

- `GET /health` basic status and model name
- `GET /v1/models` list available models
- `POST /v1/chat/completions` streaming chat responses (SSE only)
- `POST /v1/completions` text completions (non-chat)
- `POST /v1/embeddings` embeddings only (enabled when `server.embeddings: true`)
- `POST /v1/chat/hibernate` persist and unload a session from memory
- `POST /v1/chat/delete` delete a session permanently

## Headers

- `Authorization: Bearer <api_key>` if `server.api_key` is set
- `X-Session-ID: <id>` enables stateful sessions
- `X-Fresh-Session: true|false` hints whether the session is new (default `false` if `X-Session-ID` is present; `true` otherwise)
- `X-Session-Fork: <template>` (with `X-Fresh-Session: true`) copies `<template>.bin` to the new session ID, then treats the request as a resume so the engine loads the copied cache

## Chat Completions Example

```bash
curl http://127.0.0.1:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "X-Session-ID: demo" \
  -d '{
    "model": "my-assistant",
    "stream": true,
    "messages": [
      {"role": "user", "content": "Hello"}
    ]
  }'
```

Responses stream as SSE (`data: ...`) with a final `data: [DONE]` chunk.

## Completions Notes

- `POST /v1/completions` supports both streaming (`stream: true`) and non-streaming.
- Streaming only supports a single prompt per request.

## Error Notes

- A `409` response indicates a missing session cache; resend full history.
- If the server is in embeddings-only mode, chat endpoints return a `404` (Not Found).
