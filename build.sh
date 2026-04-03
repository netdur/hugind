#!/bin/bash

# Compile the hugind executable
set -euo pipefail

echo "Building hugind..."
if command -v rg >/dev/null 2>&1; then
  VERSION=$(rg -n "^version\\s*=\\s*\"([^\"]+)\"" Cargo.toml -o --replace '$1' | head -n1)
else
  VERSION=$(grep -E '^version[[:space:]]*=' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/' | head -n1)
fi
echo "Detected version: $VERSION"

cargo update -p llama-cpp 2>/dev/null || true
cargo build --release --features metal
echo "Build successful! Binary is at target/release/hugind"
