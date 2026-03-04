# llama.cpp Rust Binding Support Checklist

Date: 2026-03-01  
Scope: `src/resources/config_2.yml` vs current Hugind runtime path.

## Goal

Track which config options are:

- exposed by `llama_cpp` FFI
- represented in Hugind wrapper structs
- actually parsed/wired into runtime

Use this as the implementation checklist while adding support.

## Sources Checked

- `src/resources/config_2.yml`
- `src/core/config/server.rs`
- `src/core/config/loader.rs`
- `src/cli/server.rs`
- `src/server/llm/model/params.rs`
- `src/server/llm/context/params.rs`
- `src/server/llm/sampling/config.rs`
- `src/server/llm/sampling/mod.rs`
- `src/server/llm/multimodal/mod.rs`
- Generated FFI bindings:
  - `target/debug/build/llama-cpp-ffi-274cbd564b69eca0/out/bindings.rs`

## Status Legend

- `FFI`: Available in generated `llama_cpp` bindings.
- `Wrapper`: Exists in Hugind wrapper struct (`src/server/llm/*/params.rs` etc.).
- `Wired`: Parsed and actually applied in current `hugind server start` runtime path.

## 1) Model Options (`model.*`)

| Option | FFI | Wrapper | Wired | Notes |
|---|---|---|---|---|
| `gpu_layers` | yes (`n_gpu_layers`) | yes | yes | Applied in `src/cli/server.rs`. |
| `split_mode` | yes | yes | no | Parsed but not assigned to runtime `mparams`. |
| `main_gpu` | yes | yes | yes | Applied. |
| `tensor_split` | yes | yes | no | Wrapper field exists, not parsed/wired. |
| `vocab_only` | yes | no | no | Parsed in core config, but runtime wrapper lacks field. |
| `use_mmap` | yes | yes | yes | Applied. |
| `use_direct_io` | yes | no | no | FFI supports, wrapper missing. |
| `use_mlock` | yes | yes | yes | Applied. |
| `check_tensors` | yes | yes | no | Wrapper has field, config/runtime path does not wire it. |
| `use_extra_bufts` | yes | no | no | FFI supports, wrapper missing. |
| `no_host` | yes | no | no | FFI supports, wrapper missing. |
| `no_alloc` | yes | no | no | FFI supports, wrapper missing. |
| `devices` | yes | no | no | FFI supports explicit device list. |
| `tensor_buft_overrides` | yes | no | no | FFI pointer field exposed. |
| `kv_overrides` | yes | no | no | FFI pointer field exposed. |

## 2) Context Options (`context.*`)

| Option | FFI | Wrapper | Wired | Notes |
|---|---|---|---|---|
| `size` (`n_ctx`) | yes | yes | yes | Applied. |
| `batch_size` (`n_batch`) | yes | yes | yes | Applied. |
| `ubatch_size` (`n_ubatch`) | yes | yes | yes | Applied. |
| `seq_max` | yes | yes | partial | Runtime currently forces `max_slots` path. |
| `threads` | yes | yes | no | Parsed, not assigned in `src/cli/server.rs`. |
| `threads_batch` | yes | yes | no | Parsed, not assigned. |
| `rope_scaling_type` | yes | yes | no | Wrapper has field, no parse/wire. |
| `pooling_type` | yes | yes | partial | Forced to mean only when embeddings enabled. |
| `attention_type` | yes | no | no | FFI has it, wrapper missing. |
| `flash_attention` (bool alias) | indirect (`flash_attn_type`) | no | no | Needs mapping policy. |
| `flash_attn_type` | yes | no | no | FFI has enum, wrapper missing. |
| `rope_freq_base` | yes | yes | no | Wrapper field exists, not parsed/wired. |
| `rope_freq_scale` | yes | yes | no | Same. |
| `yarn_ext_factor` | yes | yes | no | Same. |
| `yarn_attn_factor` | yes | yes | no | Same. |
| `yarn_beta_fast` | yes | yes | no | Same. |
| `yarn_beta_slow` | yes | yes | no | Same. |
| `yarn_orig_ctx` | yes | yes | no | Same. |
| `cache_type_k` | yes (`type_k`) | yes | no | Parsed but not assigned to runtime cparams. |
| `cache_type_v` | yes (`type_v`) | yes | no | Parsed but not assigned. |
| `offload_kqv` | yes | yes | no | Parsed but not assigned. |
| `kv_unified` | yes | no | no | FFI has it, wrapper missing. |
| `swa_full` | yes | no | no | FFI has it, wrapper missing. |
| `op_offload` | yes | no | no | FFI has it, wrapper missing. |
| `embeddings` | yes | yes | yes | Wired via `server.embeddings`. |
| `no_perf` | yes | yes | no | Wrapper has field, not parsed/wired. |
| `defrag_thold` | yes | yes | no | Wrapper has field, not parsed/wired. |
| `context_shift` | no (app logic) | n/a | partial | Implemented in engine logic, not pure params field. |
| `n_keep` | no (request/runtime policy) | n/a | partial | Exists in request params; needs policy wiring. |
| `n_discard` | no (request/runtime policy) | n/a | partial | Exists in request params; needs policy wiring. |
| `ctx_checkpoints` | no (Hugind feature) | n/a | no | No current wiring. |
| `cache_ram_mib` | no (Hugind feature) | n/a | no | No current wiring. |

## 3) Sampling Options (`sampling.*`)

### 3.1 FFI availability

FFI exposes samplers for: `top_k`, `top_p`, `min_p`, `typical`, `temp`, `temp_ext`, `xtc`, `top_n_sigma`, `mirostat`, `penalties`, `dry`, `adaptive_p`, `logit_bias`, grammar variants.

### 3.2 Current Hugind state

| Option | FFI | Wrapper | Wired | Notes |
|---|---|---|---|---|
| `temp` | yes | yes | yes | Used in sampler chain. |
| `top_k` | yes | yes | yes | Used. |
| `top_p` | yes | yes | yes | Used. |
| `min_p` | yes | yes | no | Present in config struct, not used in chain. |
| `typical_p` | yes | no | no | Missing wrapper/wiring. |
| `top_n_sigma` | yes | no | no | Missing wrapper/wiring. |
| `dynatemp_range` / `dynatemp_exp` | yes (`temp_ext`) | no | no | Missing wrapper/wiring. |
| `repeat_last_n` | yes (penalties) | yes | no | Not applied in current chain. |
| `repeat_penalty` | yes | yes | partial | Settable from request frequency penalty hack; not full config wiring. |
| `frequency_penalty` | yes | yes | no | Not applied as penalty sampler param. |
| `presence_penalty` | yes | yes | no | Not applied. |
| `dry_*` | yes | partial (core config only) | no | Not applied in chain. |
| `xtc_probability` / `xtc_threshold` | yes | partial (core config only) | no | Not applied in chain. |
| `adaptive_target` / `adaptive_decay` | yes | no | no | Missing wrapper/wiring. |
| `mirostat*` | yes | no | no | Missing wrapper/wiring. |
| `logit_bias` | yes | no | no | Missing wrapper/wiring. |
| `grammar` | yes | yes | partial | JSON grammar path works from request. |
| `json_schema*` | app-layer transform | no | no | Needs schema->grammar translation path. |
| `ignore_eos` | app/sampler policy | no | no | Needs explicit implementation. |
| `seed` | yes (`dist` etc.) | no | partial | Hardcoded `1234` currently for `dist`. |
| `samplers` / `sampler_seq` | app-chain policy | no | no | No chain builder from config yet. |
| `backend_sampling` / `no_perf` | partial | no | no | No wiring today. |

## 4) Multimodal Options (`multimodal.*`)

| Option | FFI | Wrapper | Wired | Notes |
|---|---|---|---|---|
| `enabled` | n/a | n/a | partial | Effective gate is `model.mmproj_path`. |
| `mmproj_auto` | app policy | no | no | Not implemented. |
| `mmproj_offload` | backend policy | no | no | Not implemented. |
| `image_min_tokens` | yes (`mtmd_context_params`) | no | no | FFI supports; `from_file` uses defaults only. |
| `image_max_tokens` | yes (`mtmd_context_params`) | no | no | Same. |
| `warmup` (mtmd param) | yes | no | no | Available in `mtmd_context_params`. |
| `flash_attn_type` (mtmd param) | yes | no | no | Available in `mtmd_context_params`. |

## 5) Chat / Lora / Fit / Quantize / Advanced

| Group | Current State |
|---|---|
| `chat.format` | Parsed in core config, not used in active server request path. |
| `chat.enable_thinking_default` | Not parsed/wired. |
| `lora.*` | Not parsed/wired in active path. |
| `fit.*` | Not parsed/wired; mostly llama.cpp app-layer behavior, not core params. |
| `quantize.*` | FFI has `llama_model_quantize_params`, not wired in server path. |
| `advanced.*` | Mostly llama.cpp app/common-arg runtime flags, not model/context params. |

## 6) Recommended Implementation Order

1. Align runtime wiring for fields that already exist in wrapper structs:
   - `split_mode`, `threads`, `threads_batch`, `type_k`, `type_v`, `offload_kqv`, `defrag_thold`, `no_perf`, `rope/yarn` fields.
2. Extend wrapper structs for already-exposed FFI fields:
   - model: `use_direct_io`, `use_extra_bufts`, `no_host`, `no_alloc`, `vocab_only`, overrides.
   - context: `attention_type`, `flash_attn_type`, `op_offload`, `swa_full`, `kv_unified`.
3. Rebuild sampling chain from config:
   - add penalties/min_p/dry/xtc/mirostat/adaptive/logit_bias/seed support.
4. Expose multimodal context params:
   - `image_min_tokens`, `image_max_tokens`, `warmup`, `flash_attn_type`.
5. Separate app-layer flags from core model/context params:
   - keep `fit/advanced` documented as runtime policy knobs, not direct FFI model/context fields.

## 7) Quick Validation Checklist (after each added option)

- Config parse test: field loads from YAML.
- Wrapper mapping test: Rust param -> C param value asserted.
- Runtime smoke test: launch + confirm value reflected in startup logs or behavior.
- Regression test: existing minimal config still works.
