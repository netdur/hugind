# Server Configuration (config.yml)

This document describes the server configuration format used by Hugind. It is
based on the reference files under `assets/config/`.

## File Format

- YAML document.
- The config is typically created by `hugind config init` and saved as
  `~/.hugind/configs/<name>.yml`.

## Top-Level Sections

1. `server`
2. `model`
3. `context`
4. `chat`
5. `sampling`

## `server`

Server runtime settings.

1. `host`: bind address (e.g. `0.0.0.0`).
2. `port`: TCP port.
3. `api_key`: optional Bearer token for API access.
4. `embeddings`: enables embeddings endpoint support.
6. `max_slots`: max concurrent sessions per replica.
8. `system_prompt_file`: default system prompt file.
9. `session_home`: directory to persist session state.

## `model`

Model loading options.

1. `path`: path to a `.gguf` model (absolute or registry-relative).
2. `mmproj_path`: optional vision projector path.
3. `gpu_layers`: number of layers to offload (`-1`/`99` for all).
4. `split_mode`: multi-GPU split mode (`none`, `layer`, `row`).
5. `main_gpu`: primary GPU index.
6. `use_mmap`: memory-map the model for faster loading.
7. `use_mlock`: lock model in RAM.
8. `vocab_only`: load vocab only (debug).

## `context`

Context and performance options.

1. `size`: context window (`n_ctx`), `0` = model default.
2. `batch_size`: logical batch size.
3. `ubatch_size`: physical batch size.
4. `threads`: CPU threads for generation.
5. `threads_batch`: CPU threads for prompt processing.
6. `flash_attention`: enable flash attention if supported.
7. `cache_type_k`: KV cache type for K (`f16`, `q8_0`, `q4_0`).
8. `cache_type_v`: KV cache type for V (`f16`, `q8_0`, `q4_0`).
9. `offload_kqv`: keep KV cache in VRAM.

## `sampling`

Default sampling parameters (overridable per request).

1. `temp`
2. `top_k`
3. `top_p`
4. `min_p`
5. `repeat_last_n`
6. `repeat_penalty`
7. `frequency_penalty`
8. `presence_penalty`
9. `dry_multiplier`
10. `dry_base`
11. `dry_allowed_length`
12. `xtc_probability`
13. `xtc_threshold`

## Presets

Preset fragments live in:

1. `assets/config/cpu_only.yml`
2. `assets/config/cuda_dedicated.yml`
3. `assets/config/metal_unified.yml`

These are applied by `hugind config init` to overwrite fields in the base
config. Some presets include template-style placeholders such as
`{{ physical_cores - 1 }}` for thread counts.
