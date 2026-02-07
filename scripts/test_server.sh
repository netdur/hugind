#!/usr/bin/env bash
set -euo pipefail

URL="http://127.0.0.1:8080/v1/completions"
MODEL="qwen3vl-8b"
TOTAL_REQUESTS=4

PROMPTS=(
  "hello, who are you?"
  "summarize the plot of dune in one sentence"
  "write a haiku about rain"
  "what is 13 * 17?"
  "give me a fun fact about honey bees"
  "translate 'good morning' to spanish"
  "explain black holes like I'm 10"
  "list 3 benefits of daily walking"
  "write a two-line poem about the ocean"
  "what is the capital of Japan?"
  "define: latency"
  "tell me a short joke"
  "what is 2^10?"
  "name a famous painting by Van Gogh"
  "what is photosynthesis?"
  "give me a 5-word story"
  "suggest a movie genre for a rainy day"
  "what is the largest planet?"
  "write a tweet about coffee"
  "explain recursion in one sentence"
)

if [[ "${TOTAL_REQUESTS}" -gt "${#PROMPTS[@]}" ]]; then
  echo "TOTAL_REQUESTS (${TOTAL_REQUESTS}) exceeds prompts list (${#PROMPTS[@]})."
  exit 1
fi

echo "Sending ${TOTAL_REQUESTS} streaming requests to ${URL}"

for i in $(seq 0 $((TOTAL_REQUESTS - 1))); do
  prompt="${PROMPTS[$i]}"
  (
    curl -N -sS -X POST "$URL" \
      -H "Content-Type: application/json" \
      -d "{\"model\":\"${MODEL}\",\"prompt\":\"${prompt}\",\"stream\":true}" \
    > /dev/null
    printf "[%02d] done\n" "$((i+1))"
  ) &
done

wait
echo "Done."
