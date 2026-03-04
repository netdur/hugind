# Telegram Remote Agent (Modular)

This agent is split into small files so you can reuse it as a bot template.

## Files

- `main.js`: live bot loop (poll, route, send reply)
- `lib/config.js`: token/env + loop settings
- `lib/telegram_api.js`: Telegram HTTP calls
- `lib/offset_store.js`: offset persistence to `telegram_offset.txt`
- `lib/message_filter.js`: update filtering + offset math
- `lib/llm_command.js`: `/llm` command parsing + LLM JSON contract
- `lib/text.js`: shared helpers

## Customize Quickly

1. Keep `main.js` as-is.
2. Edit `lib/llm_command.js`:
   - `buildReplyForMessage(text)` controls bot behavior.
   - Add commands (`/help`, `/image`, etc.) there.
3. If you want admin-only mode:
   - Set `ADMIN_USER_ID`.
   - Filtering is enforced in `lib/message_filter.js`.
4. If you do not want offset files:
   - Replace `readOffset/writeOffset` calls in `main.js` with in-memory logic.

## Run

```bash
export TELEGRAM_BOT_TOKEN="..."
export ADMIN_USER_ID="123456789" # optional
./target/release/hugind agent run agent/telegram_remote
```
