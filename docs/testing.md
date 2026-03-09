# Testing Guide

This document describes how to run Hugind tests locally and how they map to CI.

## Prerequisites

- Rust toolchain (stable)
- Node.js 20+ and npm
- C/C++ toolchain for `llama-cpp-ffi` (clang/cmake on macOS/Linux)

## Run Everything (CI Parity)

From repository root:

```bash
cargo test --all-targets
npm --prefix agent/cli test
```

These are the same logical checks as `.github/workflows/ci.yml`:

- Rust job: `cargo test --all-targets`
- Agent CLI job: `npm ci && npm test` in `agent/cli`

## Fast Local Loops

Rust compile-only check:

```bash
cargo check --all-targets
```

Targeted Rust module tests:

```bash
cargo test --lib core::wasm::runtime::tests -- --nocapture
cargo test --lib core::js::capabilities::net::tests -- --nocapture
cargo test --lib core::js::capabilities::shell::tests -- --nocapture
```

CLI-only tests:

```bash
npm --prefix agent/cli run test:unit
npm --prefix agent/cli run test:wasm
```

## Notes

- On macOS, some shell tests are intentionally platform-gated because `sandbox-exec` behavior differs from Linux.
- The repo includes a root `build.rs` that links `libcommon.a` from `llama-cpp-ffi` build output so tests link correctly with the current Git binding.

