# Gemma 4 Thinking Mode Support (Research)

## TL;DR
Hugind currently assumes Qwen-style thinking tags (`<think>...</think>`) across server grammar, runtime budget enforcement, and agent output cleanup. Gemma 4 thinking uses different markers (`<|channel>thought ... <channel|>`), so `enable_thinking` is not truly compatible today for Gemma 4.

To support Gemma 4 thinking mode correctly, we need to make thinking tags model-aware instead of hardcoded.

## Current State (What Works Today)

### 1) Request/API surface is generic
- `enable_thinking` and `thinking_budget_tokens` are model-agnostic in request types:
  - `src/server/types.rs:15-18`
- Server passes `enable_thinking` into chat template rendering:
  - `src/server/llm/chat/mod.rs:54-58`

### 2) Runtime behavior is hardcoded to `<think>`
- Engine marker detection is fixed to `<think>` / `</think>`:
  - `src/server/engine/mod.rs:184-187`
- Thinking grammars are hardcoded to `</think>`:
  - `src/server/routes.rs:55-59`
  - `src/server/routes.rs:87-91`
- Budget enforcement relies on detected tag tokens from those fixed markers:
  - `src/server/engine/request.rs:85-170`
  - `src/server/engine/mod.rs:1090-1098`
- Agentic output cleanup strips only `<think>...</think>`:
  - `src/core/orchestrator/agentic.rs:269-290`
  - used in `src/core/orchestrator/runner.rs:665-667`
- WASM CLI spinner/parser also only tracks `<think>`:
  - `agent/cli/main.ts:29-31`
  - `agent/cli/main.ts:73-126`

### 3) Gemma tool-calls are already supported
- Agentic parser already handles Gemma tool-call tags:
  - `src/core/orchestrator/agentic.rs:97-113`
  - tests at `src/core/orchestrator/agentic.rs:377-402`

## External Dependency Evidence (Pinned llama.cpp fork)
Current lockfile pins:
- `llama-cpp` / `llama-cpp-ffi` from `netdur/llama_cpp_rust` commit `9f37255...`
  - `Cargo.lock:1878-1889`

In that pinned vendor, Gemma4 thinking markers are:
- `thinking_start_tag = "<|channel>thought"`
- `thinking_end_tag   = "<channel|>"`
  - `/Users/adel/.cargo/git/checkouts/llama_cpp_rust-d81288ff69289262/9f37255/llama-cpp-ffi/vendor/llama.cpp/common/chat.cpp:1095-1097`

So Hugind’s hardcoded `</think>` assumption is incompatible with Gemma4’s end tag.

## Why Gemma 4 Thinking Fails in Hugind Today
1. Grammar in `routes.rs` enforces `</think>` when thinking is enabled.
2. Gemma 4 emits/uses `<|channel>thought ... <channel|>`.
3. Engine budget detector tokenizes only `<think>` tags, so budget logic may not trigger correctly for Gemma tags.
4. Agent cleanup/spinner logic does not recognize Gemma thought markers, so hidden reasoning may leak or UI behavior becomes inconsistent.

## What It Would Take

## Phase 1 (Core server correctness)

### A) Introduce model-aware thinking markers
Add a shared struct, e.g.:
- `ThinkingMarkers { open: String, close: String }`

Use it in:
- route grammar construction
- engine marker tokenization/budget handling
- any response post-processing that strips thinking text

### B) Determine markers for current model
Two viable approaches:

1. Fast path (least invasive):
- Add deterministic marker selection with defaults:
  - default: `<think>` / `</think>`
  - gemma4: `<|channel>thought` / `<channel|>`
- Detect gemma4 from model metadata/template source pattern.

2. Robust path (preferred long-term):
- Extend `llama_cpp_rust` bridge to return chat params metadata (supports_thinking + start/end tags), not only rendered prompt.
- Use those returned tags directly, removing model-family heuristics.

### C) Replace hardcoded thinking grammar constants
Refactor:
- `JSON_THINKING_GRAMMAR`
- `PLAIN_THINKING_GRAMMAR`

into a builder function that takes `close_tag` and (optionally) `budget`.

Reason:
- Gemma close tag is not `</think>`.
- Future models may use yet another marker format.

### D) Feed marker tokens into budget state
Current engine startup computes markers globally as `<think>` (`src/server/engine/mod.rs:184-201`).
Need to compute tokenized markers from selected model markers and use those in:
- forced close logic
- fallback-without-open logic

## Phase 2 (Agent/runtime correctness)

### A) Generalize `strip_thinking()`
`src/core/orchestrator/agentic.rs` should strip configurable marker pairs, not only `<think>`.

### B) Update WASM CLI thinking parser/spinner
`agent/cli/main.ts` should support marker pairs list/config, including Gemma4 tags.

This avoids:
- thinking text leaking into visible output
- spinner never ending because close tag not found

## Phase 3 (Tests + docs)

### Tests to add/update
1. Server grammar tests (`src/server/routes.rs`):
- when markers are gemma4-style, grammar includes `<channel|>` close tag
- budget prefix still injected

2. Engine budget tests (`src/server/engine/request.rs` / `mod.rs`):
- budget closes correctly for gemma markers
- fallback behavior still correct

3. Agentic strip tests (`src/core/orchestrator/agentic.rs`):
- strip Gemma thought blocks
- keep tool calls intact after stripping

4. WASM CLI tests (`agent/cli/test/wasm.spec.mjs`):
- fragmented gemma markers drive spinner correctly
- no thought text printed

### Docs to update
- `docs/http_api.md` should stop implying `<think>` is universal.
- `tool_calling_flow.md` section 4a should mention model-specific thinking markers.

## Suggested Implementation Strategy

## Option A: Minimal, shipping quickly (recommended immediate)
1. Add marker abstraction in Hugind.
2. Implement gemma4 marker detection heuristic.
3. Refactor routes/engine/orchestrator/CLI to use selected markers.
4. Add targeted tests.

Pros:
- Fast
- Unblocks Gemma4 thinking support now

Cons:
- Maintains model-specific heuristic logic in Hugind

## Option B: Proper foundation (recommended after A)
1. Extend `llama_cpp_rust` bridge to expose thinking markers derived by llama.cpp chat parser.
2. Replace Hugind heuristics with metadata from bridge.

Pros:
- Future-proof for other thinking models (Ministral/Kimi/etc.)
- Avoids repeated per-model hacks

Cons:
- Requires dependency/FFI changes and coordination

## Effort Estimate
- Option A: ~1-2 days including tests.
- Option B: +1-2 extra days (FFI + wrapper API + integration tests).

## Risks / Edge Cases
- Grammar generation for arbitrary close tags must escape special chars correctly.
- Some models emit only closing marker first; fallback logic must remain safe.
- Thinking markers can be tokenized differently across vocabularies; detection should keep current `parse_special` fallback approach.

## Bottom Line
Gemma 4 thinking support is not a single switch. The server currently hardcodes Qwen-style tags at multiple layers. The required work is to make thinking markers model-aware end-to-end (grammar, budget enforcement, and output stripping). Once that is done, Gemma4 thinking mode should work with existing `enable_thinking` + `thinking_budget_tokens` API semantics.
