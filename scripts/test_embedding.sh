#!/bin/bash

SERVER_URL="http://localhost:8082/v1/embeddings"
MODEL="nomic-embed-text-v1.5"
INPUT="The quick brown fox jumps over the lazy dog."

echo "Testing Embeddings"
echo "Target: $SERVER_URL"
echo "Model: $MODEL"
echo "Input: $INPUT"
echo "-------------------------------------"

curl -s -X POST "$SERVER_URL" \
  -H "Content-Type: application/json" \
  -d "{
    \"model\": \"$MODEL\",
    \"input\": \"$INPUT\"
  }" | jq .
