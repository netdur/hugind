#!/usr/bin/env bash
set -euo pipefail

# Long stateful chat test (non-stream JSON response).
# In stateful mode, send only the new turn each request.
# Required tools: curl, jq

SERVER_URL="${HUGIND_SERVER_URL:-http://localhost:8080/v1/chat/completions}"
MODEL="${HUGIND_MODEL:-gemma-3-4b-it}"
TURNS="${HUGIND_TURNS:-100}"
MAX_TOKENS="${HUGIND_MAX_TOKENS:-4096}"
TEMPERATURE="${HUGIND_TEMPERATURE:-0.8}"
TOP_P="${HUGIND_TOP_P:-0.9}"
SESSION_ID="${HUGIND_SESSION_ID:-debug-long-chat-$(date +%s)}"
ENABLE_THINKING="${HUGIND_ENABLE_THINKING:-false}"
SYSTEM_PROMPT="${HUGIND_SYSTEM_PROMPT:-You are a concise assistant. Keep each answer to 2 short sentences and include one emoji.}"

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required but was not found in PATH." >&2
  exit 1
fi

echo "Testing Chat Completion (Long Stateful JSON Chat)"
echo "Target:        $SERVER_URL"
echo "Model:         $MODEL"
echo "Turns:         $TURNS"
echo "Session ID:    $SESSION_ID"
echo "Thinking:      $ENABLE_THINKING"
echo "-------------------------------------"

for ((i=1; i<=TURNS; i++)); do
  USER_PROMPT="Turn $i/$TURNS. Keep continuity with prior turns. Mention at least one fact from earlier turns. Current task: summarize this sequence number and reply with one emoji."

  if (( i == 1 )); then
    MESSAGES=$(
      jq -cn --arg system "$SYSTEM_PROMPT" --arg prompt "$USER_PROMPT" \
        '[{"role":"system","content":$system},{"role":"user","content":$prompt}]'
    )
  else
    MESSAGES=$(
      jq -cn --arg prompt "$USER_PROMPT" \
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

  HTTP_BODY_FILE="$(mktemp)"
  HTTP_STATUS="$(
    curl -sS -o "$HTTP_BODY_FILE" -w "%{http_code}" -X POST "$SERVER_URL" \
      -H "Content-Type: application/json" \
      -H "x-session-id: $SESSION_ID" \
      -d "$PAYLOAD"
  )"
  RESP="$(cat "$HTTP_BODY_FILE")"
  rm -f "$HTTP_BODY_FILE"

  if [[ "$HTTP_STATUS" -ge 400 ]]; then
    if jq -e . >/dev/null 2>&1 <<<"$RESP"; then
      ERR_CODE="$(jq -r '.error.code // ""' <<<"$RESP")"
      ERR_MSG="$(jq -r '.error.message // ""' <<<"$RESP")"
      if [[ "$ERR_CODE" == "context_shift_unsupported" ]]; then
        echo
        echo "[turn $i] HTTP $HTTP_STATUS context reached: shifting unsupported by model."
        echo "server: $ERR_MSG"
        echo "Session ID: $SESSION_ID"
        exit 0
      fi
      echo
      echo "[turn $i] HTTP $HTTP_STATUS error code=${ERR_CODE:-none}"
      echo "server: ${ERR_MSG:-$RESP}"
      exit 1
    else
      echo
      echo "[turn $i] HTTP $HTTP_STATUS non-JSON error:"
      echo "$RESP"
      exit 1
    fi
  fi

  if jq -e . >/dev/null 2>&1 <<<"$RESP"; then
    CONTENT=$(jq -r '.choices[0].message.content // ""' <<<"$RESP")
    FINISH=$(jq -r '.choices[0].finish_reason // "unknown"' <<<"$RESP")
    CREATED=$(jq -r '.created // "n/a"' <<<"$RESP")
    RESPONSE_KIND="json"
  else
    CONTENT="$RESP"
    FINISH="raw"
    CREATED="n/a"
    RESPONSE_KIND="text"
  fi
  OUT_LEN=$(printf "%s" "$CONTENT" | wc -m | tr -d ' ')
  MSG_COUNT=$(jq 'length' <<<"$MESSAGES")

  if [[ -z "$CONTENT" ]]; then
    echo
    echo "[turn $i] Empty/invalid response. Raw payload:"
    echo "$RESP"
    exit 1
  fi

  echo "[turn $i] created=$CREATED finish_reason=$FINISH history_messages=$MSG_COUNT chars=$OUT_LEN response=$RESPONSE_KIND"
  echo "assistant: $CONTENT"
  echo

done

echo "Done. Final session id: $SESSION_ID"
