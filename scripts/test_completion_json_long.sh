#!/bin/bash

# Configuration
SERVER_URL="http://localhost:8080/v1/chat/completions"
MODEL="gemma-3-4b-it"
PROMPT="tell me a long story about a cat and a dog that are friends and go on adventures together in a magical forest where they meet talking animals and discover hidden treasures. the cat is clever and curious, while the dog is loyal and brave. they work together to solve puzzles and overcome challenges, learning valuable lessons about friendship, courage, and kindness along the way."

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
