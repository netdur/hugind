#!/bin/bash

# Compile the hugind executable
echo "Building hugind..."
dart compile exe bin/hugind.dart -o bin/hugind

if [ $? -eq 0 ]; then
    echo "Build successful! Executable is at bin/hugind"
else
    echo "Build failed."
    exit 1
fi
