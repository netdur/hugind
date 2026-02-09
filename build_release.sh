#!/bin/bash

# 1. Create a clean distribution directory
rm -rf dist
mkdir -p dist

# 2. Compile the Rust executable
echo "Compiling hugind..."
if command -v rg >/dev/null 2>&1; then
  VERSION=$(rg -n "^version\\s*=\\s*\"([^\"]+)\"" Cargo.toml -o --replace '$1' | head -n1)
else
  VERSION=$(grep -E '^version[[:space:]]*=' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/' | head -n1)
fi
echo "Detected version: $VERSION"
cargo build --release
cp target/release/hugind dist/hugind

# 3. Copy Configuration files
echo "Copying configs..."
if [ -d "assets/config" ]; then
  cp -r assets/config dist/
else
  echo "No config directory found (assets/config, bin/config, or config). Skipping."
fi

# 4. Create the archive
echo "Creating tarball..."
cd dist
tar -czvf ../hugind-macos-arm64.tar.gz *
cd ..

echo "Done! hugind-macos-arm64.tar.gz is ready."
echo "SHA256: $(shasum -a 256 hugind-macos-arm64.tar.gz)"
