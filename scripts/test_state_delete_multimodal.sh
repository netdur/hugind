#!/bin/bash

set -euo pipefail

SERVER_URL="http://localhost:8081/v1/chat/completions"
STATE_DELETE_URL="http://localhost:8081/v1/state"
STATE_SAVE_URL="http://localhost:8081/v1/state/save"
STATE_IDLE_URL="http://localhost:8081/v1/state/idle"
MODEL="gemma-3-4b-it"
SESSION_ID="session_madonna_test"
TEMPLATE_ID="session_madonna_test"
IMAGE_PATH="assets/madonna.jpg"
IMAGE_B64="$(base64 < "$IMAGE_PATH" | tr -d '\n')"
IMAGE_URL="data:image/jpeg;base64,${IMAGE_B64}"
CACHE_FILE="cache/${SESSION_ID}.bin"

echo "Step 1: image question (place)"
resp1=$(curl -s -X POST "$SERVER_URL" \
  -H "Content-Type: application/json" \
  -H "X-Request-ID: $SESSION_ID" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [
      {
        \"role\": \"user\",
        \"content\": [
          {\"type\": \"text\", \"text\": \"What is the place (forest, beach, etc)?\"},
          {\"type\": \"image_url\", \"image_url\": {\"url\": \"$IMAGE_URL\"}}
        ]
      }
    ],
    \"stream\": false
  }")
echo "$resp1" | jq -r '.choices[0].message.content'
echo

echo "Step 2: idle session state (vram -> ram)"
curl -s -X POST "$STATE_IDLE_URL" \
  -H "Content-Type: application/json" \
  -d "{
    \"session_id\": \"$SESSION_ID\"
  }"
echo
sleep 0.2

echo "Step 3: save session state (ram -> disk)"
curl -s -X POST "$STATE_SAVE_URL" \
  -H "Content-Type: application/json" \
  -d "{
    \"session_id\": \"$SESSION_ID\",
    \"template_id\": \"$TEMPLATE_ID\"
  }"
echo
sleep 0.2

echo "Step 4: verify cache file exists and size"
if [ -f "$CACHE_FILE" ]; then
  ls -lh "$CACHE_FILE"
else
  echo "File missing: $CACHE_FILE"
fi
echo

echo "Step 5: text-only question (hair color)"
resp2=$(curl -s -X POST "$SERVER_URL" \
  -H "Content-Type: application/json" \
  -H "X-Request-ID: $SESSION_ID" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"What is the color of her hair?\"}],
    \"stream\": false
  }")
echo "$resp2" | jq -r '.choices[0].message.content'
echo

echo "Step 6: delete session state (free vram/ram + delete file)"
curl -s -X DELETE "$STATE_DELETE_URL/$SESSION_ID"
echo

echo "Step 7: verify cache file removed"
if [ -f "$CACHE_FILE" ]; then
  echo "File still present: $CACHE_FILE"
  ls -lh "$CACHE_FILE"
else
  echo "File removed: $CACHE_FILE"
fi
