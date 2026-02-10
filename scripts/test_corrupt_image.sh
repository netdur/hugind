#!/bin/bash

SERVER_URL="http://localhost:8081/v1/chat/completions"
MODEL="gemma-3-4b-it"
CORRUPT_IMAGE_DATA_URL="data:image/jpeg;base64,@@not_base64@@"
PROMPT="Describe this image."

echo "Testing Multimodal Chat Completion with Corrupt Image"
echo "Target: $SERVER_URL"
echo "Model: $MODEL"
echo "Corrupt Image: data URL (invalid base64)"
echo "Prompt: $PROMPT"
echo "-------------------------------------"

RESP_FILE="$(mktemp)"
HTTP_CODE="$(curl -sS -o "$RESP_FILE" -w "%{http_code}" -X POST "$SERVER_URL" \
  -H "Content-Type: application/json" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [
      {
        \"role\": \"user\",
        \"content\": [
          {\"type\": \"text\", \"text\": \"$PROMPT\"},
          {\"type\": \"image_url\", \"image_url\": {\"url\": \"$CORRUPT_IMAGE_DATA_URL\"}}
        ]
      }
    ],
    \"stream\": false
  }")"

echo "HTTP $HTTP_CODE"
if jq -e . >/dev/null 2>&1 < "$RESP_FILE"; then
  cat "$RESP_FILE" | jq .
else
  echo "Non-JSON response:"
  cat "$RESP_FILE"
fi

rm -f "$RESP_FILE"
