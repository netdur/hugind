#!/bin/bash

# 1. Create a clean distribution directory
rm -rf dist
mkdir -p dist

# 2. Compile the Dart executable
echo "Compiling hugind..."
VERSION=$(grep 'version:' pubspec.yaml | sed 's/version: //')
echo "Detected version: $VERSION"
dart compile exe bin/hugind.dart -DVERSION=$VERSION -o dist/hugind

# 3. Copy Configuration files
echo "Copying configs..."
cp -r bin/config dist/

# 4. Copy Shared Libraries (The dylibs)
# Adjust the path below to match where your dylibs currently reside
DYLIBS_PATH="/Users/adel/Workspace/llama_cpp_dart/bin/MAC_ARM64"
echo "Copying libraries from $DYLIBS_PATH..."
cp $DYLIBS_PATH/*.dylib dist/

# 5. Create the archive
echo "Creating tarball..."
cd dist
tar -czvf ../hugind-macos-arm64.tar.gz *
cd ..

echo "Done! hugind-macos-arm64.tar.gz is ready."
echo "SHA256: $(shasum -a 256 hugind-macos-arm64.tar.gz)"