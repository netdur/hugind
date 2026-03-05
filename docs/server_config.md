# Server Configuration (`config.yml`)

This document describes Hugind server config parsing as implemented in
`src/core/config/loader.rs` and `src/core/config/server.rs`.

## File Location

- Config files are typically written by `hugind config init` to:
  `config_home()/configs/<name>.yml`
- On Unix-like systems this is usually:
  `$XDG_CONFIG_HOME/hugind/configs` or `~/.hugind/configs`

## Top-Level Sections

Supported sections:

1. `server` (optional)
2. `model`
3. `context`
4. `multimodal`
5. `sampling`
6. `chat`
7. `lora`
8. `fit`
9. `quantize`
10. `advanced`

The base template in `src/resources/config.yml` includes all except `server`
(which is optional and uses defaults when omitted).

## `server` Section

Runtime service settings:

1. `host` (default `0.0.0.0`)
2. `port` (default `8080`)
3. `api_key` (optional bearer token)
4. `max_slots` (defaults to `context.seq_max`; applied back to context)
5. `system_prompt` (default `"You are a helpful assistant."`)
6. `system_prompt_file` (optional path; if readable, content overrides
   `system_prompt`)
7. `embeddings` (boolish; defaults to `context.embeddings`)
8. `session_home` (optional path; defaults to `paths::sessions_dir()`)
9. `unified_memory_mode` (boolish; default `false`)
10. `verbose` (boolish; default `false`)

## `model` Section

Model identity and loading parameters:

1. `path` (model `.gguf` path)
2. `name` (optional public model name)
3. `mmproj_path` (optional vision projector path)
4. model params such as `gpu_layers`, `split_mode`, `main_gpu`, `tensor_split`,
   `use_mmap`, `use_mlock`, etc.

Notes:

- `gpu_layers` also accepts alias `n_gpu_layers`.
- Relative paths are resolved relative to the config file directory.
- `~` in paths is expanded to home directory.

## `context` Section

Context/runtime parameters:

1. `size` (`n_ctx` alias)
2. `batch_size` (`n_batch` alias)
3. `ubatch_size` (`n_ubatch` alias)
4. `seq_max` (`n_seq_max` alias)
5. `threads` / `threads_batch`
6. attention/rope/pooling options
7. KV cache settings (`cache_type_k`, `cache_type_v`, `offload_kqv`, etc.)

Notes:

- `flash_attention: true` auto-sets `flash_attn_type` to `on` when currently
  `auto`.
- For vision models (`mmproj_path` set), very low `batch_size` is auto-raised
  to `8192`.

## `multimodal`, `sampling`, `chat`, `lora`, `fit`, `quantize`, `advanced`

These sections map directly to strongly-typed structs in
`src/core/config/server.rs`. The canonical key set is in
`src/resources/config.yml`.

Notable `chat` fields:

1. `enable_thinking_default`
2. `thinking_budget_tokens`
3. `format` (legacy chat-format compatibility field)

## Boolish Parsing

For `server.embeddings`, `server.unified_memory_mode`, and `server.verbose`,
Hugind accepts booleans and common strings:

- true-ish: `true`, `on`, `yes`, `enabled`, `1`
- false-ish: `false`, `off`, `no`, `disabled`, `0`

## Presets

`hugind config init` overlays preset fragments from:

1. `src/resources/cpu_only.yml`
2. `src/resources/cuda_dedicated.yml`
3. `src/resources/metal_unified.yml`

onto the base template `src/resources/config.yml`.
