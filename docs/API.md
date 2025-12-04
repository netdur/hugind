# Hugind API Compatibility

OpenAI-compatible endpoints exposed by the Hugind server, plus unsupported ones for clarity.

## Summary
| Endpoint | Purpose | Hugind | vLLM |
| --- | --- | --- | --- |
| `GET /health` | Liveness probe (non-OpenAI) | Yes | No |
| `GET /v1/models` | List model ids | Yes | Yes |
| `POST /v1/chat/completions` | Chat generation | Yes | Yes |
| `POST /v1/completions` | Legacy completion API | Yes | Yes |
| `POST /v1/embeddings` | Vector embeddings | Yes* | Yes |
| `POST /v1/audio/transcriptions` | Speech-to-text | No | No |
| `POST /v1/audio/translations` | Speech translation | No | No |
| `POST /v1/images/generations` | Image generation | No | No |
| `POST /v1/images/edits` | Image edit | No | No |
| `POST /v1/images/variations` | Image variations | No | No |
| `POST /v1/fine_tuning/jobs` | Fine-tuning | No | No |
| `GET /v1/fine_tuning/jobs` | List fine-tunes | No | No |
| `POST /v1/moderations` | Content moderation | No | No |
| `POST /v1/batches` | Batch jobs | No | No |
| `/v1/assistants/*` | Assistants API surface | No | No |

### `/v1/completions` Feature Parity
| Feature/Field | OpenAI Behavior | vLLM | Hugind |
| --- | --- | --- | --- |
| Prompt (`prompt`) | Required text/array | Yes | Yes |
| Model (`model`) | Selects model id | Yes | Yes |
| Max tokens (`max_tokens`) | Truncates output | Yes | Stub (ignored) |
| Temperature/`top_p`/`top_k` | Sampling controls | Yes | Stub (ignored) |
| Stop sequences (`stop`) | Halts on match | Yes | Stub (ignored) |
| Logprobs (`logprobs`, `top_logprobs`) | Token probabilities | Yes | No |
| `echo` | Returns prompt tokens | Yes | No |
| `n` (multiple choices) | Generate N completions | Yes | Partial (non-stream only, sequential) |
| Streaming (`stream`) | SSE chunks when true | Yes | Yes (single prompt only) |
| `presence_penalty`/`frequency_penalty` | Penalize prior tokens | Yes | No |
| JSON mode (`response_format`) | Constrained JSON | Partial | No |
| Tool/function calling | Not in completions | N/A | N/A |
| Attachments | Not part of endpoint | N/A | N/A |

## Plan to Close Gap with vLLM
| Item | Current State | Next Steps |
| --- | --- | --- |
| Auth parity | Config field unused | Enforce `api_key` in handlers and document header format. |
| Error/compat polish | Partial | Align response fields/codes with OpenAI spec across endpoints; add token usage accounting. |
| Sampling parity | Partial | Wire `max_tokens`, `stop`, temperature/top_p/top_k into sampler params. |
| Completions parity | Partial | Add `logprobs`, `echo`, proper `n` fan-out, and JSON mode if desired. |
| Embeddings polish | Implemented | Document batching/limits and model selection; consider usage tokens. |

## Behavior Notes
- Authentication: Config supports `server.api_key`, but current handlers do not enforce it; run behind your own auth/reverse proxy if needed.
- Model id: The `model` field in requests should match the config name used at startup (e.g., `tiny-llama`).
- Sessions: Providing a stable `user` string reuses cached context across requests.
- Streaming: `/v1/chat/completions` always streams SSE chunks; `/v1/completions` supports both streaming (`stream: true`) and buffered responses.
- Embeddings: `/v1/embeddings` is available only when the server is started in embeddings-enabled mode (`server.embeddings: true` in config).

## Examples / Tests
See integration-style examples in `test/api/*.dart`. They are marked `skip` by default; run with a live server using `--run-skipped`, e.g.:
```
HUGIND_URL=http://127.0.0.1:8080 dart test test/api/completions_test.dart --run-skipped
```
