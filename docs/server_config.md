# Server Configuration (`config.yml`)

This document describes Hugind server config parsing as implemented in
`src/core/config/loader.rs` and `src/core/config/server.rs`.

## File Location

Config files are written by `hugind config init` to `~/.hugind/configs/<name>.yml`.

## Top-Level Sections

1. `server`
2. `model`
3. `context`
4. `multimodal`
5. `sampling`
6. `lora`
7. `fit`
8. `quantize`
9. `advanced`

The base template is in `src/resources/config.yml`.

## `server` Section

1. `host` (default `0.0.0.0`)
2. `port` (default `8080`)
3. `api_key` (optional bearer token)
4. `max_slots` (defaults to `context.seq_max`)
5. `system_prompt` (default `"You are a helpful assistant."`)
6. `system_prompt_file` (optional path; errors if unreadable)
7. `embeddings` (boolish; default `false`)
8. `session_home` (optional path; defaults to `~/.hugind/sessions`)
9. `unified_memory_mode` (boolish; default `false`)
10. `verbose` (boolish; default `false`)
11. `enable_thinking_default` (boolish; default `false`)
12. `thinking_budget_tokens` (optional u32; null = no cap)

## `model` Section

1. `path` (model `.gguf` path)
2. `name` (optional public model name)
3. `mmproj_path` (optional vision projector path)
4. Model params: `gpu_layers`, `split_mode`, `main_gpu`, `tensor_split`,
   `use_mmap`, `use_mlock`, etc.

Notes:
- `gpu_layers` also accepts alias `n_gpu_layers`.
- Relative paths resolved relative to the config file directory.
- `~` in paths is expanded to home directory.

## `context` Section

1. `size` (`n_ctx`)
2. `batch_size` (`n_batch`)
3. `ubatch_size` (`n_ubatch`)
4. `seq_max` (`n_seq_max`)
5. `threads` / `threads_batch`
6. Attention/rope/pooling options
7. KV cache settings (`cache_type_k`, `cache_type_v`, `offload_kqv`, etc.)

Notes:
- `flash_attention: true` auto-sets `flash_attn_type` to `on`.
- For vision models, low `batch_size` is auto-raised to `8192`.

## `fit` Section

1. `enabled` (default `false`)
2. `target_mib` (per-device memory targets in MiB)
3. `min_ctx` (minimum context size when fitting)

When enabled, the engine adjusts context size at startup to fit available memory.

## Other Sections

`multimodal`, `sampling`, `lora`, `quantize`, `advanced` map directly to
structs in `src/core/config/server.rs`. See `src/resources/config.yml` for
the full key set with defaults.

## Boolish Parsing

For boolish fields, Hugind accepts booleans and common strings:
- true: `true`, `on`, `yes`, `enabled`, `1`
- false: `false`, `off`, `no`, `disabled`, `0`

Unrecognized values produce a warning.

## Hardware Auto-Configuration

`hugind config init` detects hardware and auto-configures:
- GPU layers (99 for NVIDIA/Apple Silicon, 0 for CPU-only)
- Flash attention (enabled for NVIDIA)
- KV offload (enabled when GPU available)
- Thread count (optimized for CPU architecture)
- Unified memory mode (enabled for Apple Silicon)
