# Compiler War Plan

## Goal
Get `hugind` to a clean build on Rust 2024 (no local warnings), and remove known future-incompatibility risk from `rquickjs-core`.

## Current Warning Inventory
1. `unsafe_op_in_unsafe_fn` (`E0133`) in `src/server/llm/batch/fill.rs:22`, `src/server/llm/batch/fill.rs:25`, `src/server/llm/batch/fill.rs:28`, `src/server/llm/batch/fill.rs:32`, `src/server/llm/batch/fill.rs:33`, `src/server/llm/batch/fill.rs:36`, `src/server/llm/batch/fill.rs:40`.
2. `dead_code`: `McpRequest.jsonrpc` never read in `src/stdio/mod.rs:28`.
3. `dead_code`: `EventMode::None` never constructed in `src/stdio/mod.rs:463`.
4. Cargo future-incompat note: `rquickjs-core v0.6.2` (never-type fallback warning; will hard-fail in newer compilers).

## Plan of Attack

### Phase 1: Fix `unsafe_op_in_unsafe_fn` in batch fill

### Why
Rust 2024 no longer treats `unsafe fn` body as implicitly unsafe. Each raw-pointer op must be wrapped in explicit `unsafe {}`.

### Edits
1. Refactor `src/server/llm/batch/fill.rs`:
   - Keep `batch_set` as `unsafe fn` (the caller still must uphold invariants).
   - Wrap raw pointer operations in explicit `unsafe` blocks.
   - Add `// SAFETY:` comments for each block to document invariants.
   - Replace the empty `if i >= batch.n_tokens as usize {}` with a meaningful debug assertion or remove it.

2. Optional cleanup:
   - Make `batch_set` `pub(crate)` (not `pub`) because it is internal to `batch` module.
   - Collapse repeated pointer writes into small helpers to reduce unsafe surface.

### Acceptance
`cargo check` shows no `E0133` from `fill.rs`.

---

### Phase 2: Remove dead-code warning for MCP `jsonrpc` field

### Why
`is_mcp_message` checks `jsonrpc` before deserialization, but `McpRequest.jsonrpc` is still never used, so dead-code lint fires.

### Edits
1. In `src/stdio/mod.rs`, inside `handle_mcp_message` immediately after parse:
   - Validate `req.jsonrpc.as_deref() == Some("2.0")`.
   - If invalid, return JSON-RPC error `-32600` (Invalid Request).

2. This both uses the field and hardens protocol correctness.

### Acceptance
No dead-code warning for `McpRequest.jsonrpc` and invalid JSON-RPC version is handled explicitly.

---

### Phase 3: Resolve `EventMode::None` warning

### Why
`EventMode::None` plus noop emitters exist but no call path constructs it.

### Edits (recommended)
1. Remove `EventMode::None` from `src/stdio/mod.rs:460`.
2. Remove unreachable match arms in:
   - `agent.run` branch (`src/stdio/mod.rs:222`-`src/stdio/mod.rs:224`).
   - `model.add` branch (`src/stdio/mod.rs:276`-`src/stdio/mod.rs:278`).
3. Remove unused `NoopEmitter` / `NoopProgressSink` structs and impl blocks.

### Alternative
If you want to keep a "silent mode" placeholder, annotate narrowly with `#[allow(dead_code)]` on the variant and noop structs. This is faster but weaker than deleting unused code.

### Acceptance
No dead-code warnings from `EventMode` or noop emitters.

---

### Phase 4: Future-incompatibility in `rquickjs-core`

### Why
`rquickjs-core v0.6.2` is reported as future-incompatible (never-type fallback change).

### Edits
1. Upgrade `rquickjs` in `Cargo.toml` from `0.6` to a modern compatible version (`0.11` preferred).
2. Run `cargo update -p rquickjs -p rquickjs-core -p rquickjs-macro`.
3. Fix API breakages (if any) in:
   - `src/core/js/*`
   - `src/core/wasm/*` (if indirectly affected)
4. Re-run future incompat report.

### Risk
This is the only potentially non-trivial change because `rquickjs` APIs changed across major versions.

### Acceptance
`cargo report future-incompatibilities` does not list `rquickjs-core`.

---

## Execution Order
1. Phase 1 (low risk, mechanical, required for Rust 2024 hygiene).
2. Phase 2 and Phase 3 (small protocol and cleanup changes).
3. Phase 4 (dependency upgrade; isolate in separate commit).

## Verification Checklist
Run in order:

```bash
cargo check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo report future-incompatibilities
```

For focused iteration while editing:

```bash
cargo check --lib -p hugind
```

## Commit Plan
1. `fix(llm): make batch_set Rust-2024-safe unsafe blocks explicit`
2. `refactor(stdio): remove unused EventMode::None and noop emitters`
3. `fix(stdio): validate jsonrpc version in MCP requests`
4. `chore(deps): upgrade rquickjs to remove future-incompat warnings`

## Notes
- Keep each phase in a separate commit to simplify bisecting if runtime behavior changes.
- Do not silence `E0133` globally; keep explicit unsafe boundaries documented.
