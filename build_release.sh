#!/usr/bin/env bash
set -e

echo "building hugind exe"
dart compile exe bin/hugind.dart -o bin/hugind

echo "preparing dist"
rm -rf dist
mkdir -p dist/config

# 1 binary
cp bin/hugind dist/

# 2 config yml files
cp bin/config/*.yml dist/config/

# 3 llama cpp dylibs
cp /Users/adel/Workspace/llama_cpp_dart/bin/MAC_ARM64/*.dylib dist/

echo "creating archive"
tar -czf hugind-macos-arm64.tar.gz -C dist .

echo "done"
