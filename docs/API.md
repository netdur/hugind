# Hugind API Compatibility

OpenAI-compatible endpoints exposed by the Hugind server, plus unsupported ones for clarity.

## Summary
| Endpoint | Purpose | Hugind | vLLM |
| --- | --- | --- | --- |
| `GET /health` | Liveness probe (non-OpenAI) | Yes | No |
| `GET /v1/models` | List model ids | Yes | Yes |
| `POST /v1/chat/completions` | Chat generation | Yes | Yes |
| `POST /v1/completions` | Legacy completion API | No | Yes |
| `POST /v1/embeddings` | Vector embeddings | No | Yes |
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
| Prompt (`prompt`) | Required text/array | Yes | No (endpoint missing) |
| Model (`model`) | Selects model id | Yes | No |
| Max tokens (`max_tokens`) | Truncates output | Yes | No |
| Temperature/`top_p`/`top_k` | Sampling controls | Yes | No |
| Stop sequences (`stop`) | Halts on match | Yes | No |
| Logprobs (`logprobs`, `top_logprobs`) | Token probabilities | Yes | No |
| `echo` | Returns prompt tokens | Yes | No |
| `n` (multiple choices) | Generate N completions | Yes | No |
| Streaming (`stream`) | SSE chunks when true | Yes | No |
| `presence_penalty`/`frequency_penalty` | Penalize prior tokens | Yes | No |
| JSON mode (`response_format`) | Constrained JSON | Partial | No |
| Tool/function calling | Not in completions | N/A | N/A |
| Attachments | Not part of endpoint | N/A | N/A |

## Plan to Close Gap with vLLM
| Item | Current State | Next Steps |
| --- | --- | --- |
| Add `/v1/completions` | Not implemented | Reuse chat pipeline with prompt-only input/output shape; wire SSE + non-stream modes. |
| Add `/v1/embeddings` | Not implemented | Expose llama embedding mode; define model selection and batching strategy. |
| Auth parity | Config field unused | Enforce `api_key` in handlers and document header format. |
| Error/compat polish | Partial | Align response fields/codes with OpenAI spec across endpoints. |

## Behavior Notes
- Authentication: Config supports `server.api_key`, but current handlers do not enforce it; run behind your own auth/reverse proxy if needed.
- Model id: The `model` field in requests should match the config name used at startup (e.g., `tiny-llama`).
- Sessions: Providing a stable `user` string reuses cached context across requests.
- Streaming: `/v1/chat/completions` always streams SSE chunks; there is no non-streaming mode.
