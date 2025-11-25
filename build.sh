#!/bin/bash

# Compile the hugind executable
echo "Building hugind..."
VERSION=$(grep 'version:' pubspec.yaml | sed 's/version: //')
echo "Detected version: $VERSION"
dart compile exe bin/hugind.dart -DVERSION=$VERSION -o bin/hugind

if [ $? -eq 0 ]; then
    echo "Build successful! Executable is at bin/hugind"
else
    echo "Build failed."
    exit 1
fi
