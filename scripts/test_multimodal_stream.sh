#!/bin/bash

# Note: Ensure you run ./scripts/start_server_multimodal.sh on port 8081 first!

SERVER_URL="http://localhost:8080/v1/chat/completions"
MODEL="gemma-3-4b-it"
IMAGE_PATH="assets/madonna.jpg"
IMAGE_B64="$(base64 < "$IMAGE_PATH" | tr -d '\n')"
IMAGE_URL="data:image/jpeg;base64,${IMAGE_B64}"
PROMPT="Describe this image."

echo "Testing Multimodal Chat Completion (Streaming)"
echo "Target: $SERVER_URL"
echo "Model: $MODEL"
echo "Image: $IMAGE_PATH"
echo "Prompt: $PROMPT"
echo "-------------------------------------"

curl -s -N -X POST "$SERVER_URL" \
  -H "Content-Type: application/json" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [
      {
        \"role\": \"user\",
        \"content\": [
          {\"type\": \"text\", \"text\": \"$PROMPT\"},
          {\"type\": \"image_url\", \"image_url\": {\"url\": \"$IMAGE_URL\"}}
        ]
      }
    ],
    \"stream\": true
  }"
