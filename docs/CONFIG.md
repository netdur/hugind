# Configuration

Hugind configurations are YAML files stored under `<config_home>/configs/*.yml`. Each config controls server settings, model paths, context size, and sampling defaults. See `docs/cli.md` for how `<config_home>` and `<data_home>` are resolved.

## Commands

### `hugind config info`
Prints detected system hardware details and a recommended preset.

### `hugind config init <name>`
Interactive wizard that:
- Probes hardware
- Applies a preset (`metal_unified`, `cuda_dedicated`, `cpu_only`)
- Lets you select a model file
- Detects vision projectors (`mmproj`) when present
- Suggests a safe context size
- Writes the resulting config to `<config_home>/configs/<name>.yml`

Options:
- `-m, --model <path>` skip the model picker and use a specific file

### `hugind config list`
Lists saved configs by name.

### `hugind config remove <name>`
Deletes a saved config after confirmation.

### `hugind config defaults [--lib <path>] [--hf-token <token>]`
Sets global defaults in `<data_home>/settings.yml`.

- `--lib` sets the default `libllama` library path.
- `--hf-token` sets the Hugging Face token used by `hugind model add`.

If no options are provided, it prints the current defaults.

## Config File Layout

Below is a minimal example that mirrors the fields consumed by the server.

```yaml
server:
  host: "0.0.0.0"
  port: 8080
  api_key: ""
  embeddings: false
  concurrency: 1
  max_slots: 4
  timeout_seconds: 600
  system_prompt_file: "prompts/coding_assistant.txt"
  library_path: ""

model:
  path: "~/Models/llama-3-8b.gguf"
  mmproj_path: ""
  gpu_layers: 99
  split_mode: layer
  main_gpu: 0
  use_mmap: true
  use_mlock: false
  vocab_only: false

context:
  size: 4096
  batch_size: 2048
  ubatch_size: 512
  threads: 8
  threads_batch: 8
  flash_attention: false
  cache_type_k: f16
  cache_type_v: f16
  offload_kqv: true

chat:
  format: auto

sampling:
  temp: 0.7
  top_k: 40
  top_p: 0.95
  min_p: 0.05
  dry_multiplier: 0.0
```

### Notes on Key Fields

- `server.library_path` can be omitted if Hugind can auto-detect the shared library.
- `server.session_home` defaults to `<data_home>/sessions/` unless overridden.
- `model.path` is required; Hugind resolves `~` and relative paths.
- `context.nSeqMax` is derived from `server.max_slots` in code.
- `chat.format` supports `none`, `chatml`, `chatmlThinking`, `qwen3`, `gemma`, `alpaca`, `harmony`.
- `embeddings: true` enables only `/v1/embeddings` and disables chat/completions.
- Only `sampling.temp`, `sampling.top_k`, `sampling.top_p`, `sampling.min_p`, and `sampling.dry_multiplier` are consumed today.

## Best Practices

- Run `hugind config info` before `init` if you want a quick read of CPU/GPU/RAM.
- Use the wizard to keep context sizes safe; manual oversizing can cause OOM.
- For vision models, keep `batch_size` at `8192` or higher.
- If you use a custom `libllama`, set it once in defaults and omit it from configs.
