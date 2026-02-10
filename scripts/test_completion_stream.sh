#!/bin/bash

# Configuration
SERVER_URL="http://localhost:8080/v1/chat/completions"
MODEL="gemma-3-4b-it"
PROMPT="Write a short poem about coding."

echo "Testing Chat Completion (Streaming Mode)"
echo "Target: $SERVER_URL"
echo "Model: $MODEL"
echo "Prompt: $PROMPT"
echo "-------------------------------------"

curl -N -X POST "$SERVER_URL" \
  -H "Content-Type: application/json" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"$PROMPT\"}],
    \"stream\": true
  }"
