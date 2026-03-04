#!/usr/bin/env bash
set -euo pipefail

# Stateful multimodal memory-loss repro:
# 1) Turn 1 sends an image and asks hair color.
# 2) Next turns ask hair color again (no image) using same session.
# 3) Stop when answer no longer says blond/blonde.

SERVER_URL="${HUGIND_SERVER_URL:-http://localhost:8080/v1/chat/completions}"
MODEL="${HUGIND_MODEL:-gemma-3-4b-it}"
IMAGE_PATH="${HUGIND_IMAGE_PATH:-assets/madonna.jpg}"
TURNS="${HUGIND_TURNS:-300}"
MAX_TOKENS="${HUGIND_MAX_TOKENS:-64}"
TEMPERATURE="${HUGIND_TEMPERATURE:-0.2}"
TOP_P="${HUGIND_TOP_P:-0.9}"
SESSION_ID="${HUGIND_SESSION_ID:-debug-image-chat-$(date +%s)}"
ENABLE_THINKING="${HUGIND_ENABLE_THINKING:-true}"
SYSTEM_PROMPT="${HUGIND_SYSTEM_PROMPT:-Answer with one short sentence.}"

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required but was not found in PATH." >&2
  exit 1
fi

if [[ ! -f "$IMAGE_PATH" ]]; then
  echo "Image file not found: $IMAGE_PATH" >&2
  exit 1
fi

IMAGE_B64="$(base64 < "$IMAGE_PATH" | tr -d '\n')"
IMAGE_URL="data:image/jpeg;base64,${IMAGE_B64}"

echo "Testing Stateful Multimodal Chat (Image Memory)"
echo "Target:        $SERVER_URL"
echo "Model:         $MODEL"
echo "Image:         $IMAGE_PATH"
echo "Turns:         $TURNS"
echo "Session ID:    $SESSION_ID"
echo "Thinking:      $ENABLE_THINKING"
echo "Stop rule:     first answer that does NOT contain black"
echo "-------------------------------------"

FIRST_USER_PROMPT="What is the hair color of the person in this image?"
REPEAT_PROMPT="Without seeing any new image, what was the hair color of the person from the first image?"

is_black_answer() {
  local text="$1"
  local normalized
  normalized="$(printf "%s" "$text" | tr '[:upper:]' '[:lower:]')"
  if [[ "$normalized" =~ (^|[^a-z])(black)([^a-z]|$) ]]; then
    return 0
  fi
  return 1
}

for ((i=1; i<=TURNS; i++)); do
  if (( i == 1 )); then
    MESSAGES=$(
      jq -cn \
        --arg system "$SYSTEM_PROMPT" \
        --arg prompt "$FIRST_USER_PROMPT" \
        --arg image_url "$IMAGE_URL" \
        '[
          {"role":"system","content":$system},
          {
            "role":"user",
            "content":[
              {"type":"text","text":$prompt},
              {"type":"image_url","image_url":{"url":$image_url}}
            ]
          }
        ]'
    )
  else
    MESSAGES=$(
      jq -cn --arg prompt "$REPEAT_PROMPT" \
        '[{"role":"user","content":$prompt}]'
    )
  fi

  PAYLOAD=$(
    jq -cn \
      --arg model "$MODEL" \
      --argjson messages "$MESSAGES" \
      --argjson max_tokens "$MAX_TOKENS" \
      --argjson temperature "$TEMPERATURE" \
      --argjson top_p "$TOP_P" \
      --argjson thinking "$ENABLE_THINKING" \
      '{
        model: $model,
        messages: $messages,
        stream: false,
        max_tokens: $max_tokens,
        temperature: $temperature,
        top_p: $top_p,
        enable_thinking: $thinking
      }'
  )

  RESP=$(
    curl -sS -X POST "$SERVER_URL" \
      -H "Content-Type: application/json" \
      -H "x-session-id: $SESSION_ID" \
      -d "$PAYLOAD"
  )

  CONTENT="$(jq -r '.choices[0].message.content // ""' <<<"$RESP")"
  FINISH="$(jq -r '.choices[0].finish_reason // "unknown"' <<<"$RESP")"
  CREATED="$(jq -r '.created // "n/a"' <<<"$RESP")"
  OUT_LEN="$(printf "%s" "$CONTENT" | wc -m | tr -d ' ')"

  if [[ -z "$CONTENT" ]]; then
    echo
    echo "[turn $i] Empty/invalid response. Raw payload:"
    echo "$RESP" | jq .
    exit 1
  fi

  if is_black_answer "$CONTENT"; then
    BLACK_STATUS="yes"
  else
    BLACK_STATUS="no"
  fi

  echo "[turn $i] created=$CREATED finish_reason=$FINISH chars=$OUT_LEN black=$BLACK_STATUS"
  echo "assistant: $CONTENT"
  echo

  if (( i == 1 )) && [[ "$BLACK_STATUS" != "yes" ]]; then
    echo "First turn did not return black; continuing anyway for repro."
    echo
  fi

  if (( i > 1 )) && [[ "$BLACK_STATUS" == "no" ]]; then
    echo "Context likely lost at turn $i (answer is no longer black). Stopping."
    echo "Session ID: $SESSION_ID"
    exit 0
  fi
done

echo "Reached max turns ($TURNS) without losing black response."
echo "Session ID: $SESSION_ID"
