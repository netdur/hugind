#!/bin/bash

# Configuration
SERVER_URL="http://localhost:8080/v1/chat/completions"
MODEL="gemma-3-4b-it"
PROMPT="hello, who are you?"

echo "Testing Chat Completion (JSON Mode)"
echo "Target: $SERVER_URL"
echo "Model: $MODEL"
echo "Prompt: $PROMPT"
echo "-------------------------------------"

curl -s -X POST "$SERVER_URL" \
  -H "Content-Type: application/json" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"$PROMPT\"}],
    \"stream\": false
  }" | jq .
