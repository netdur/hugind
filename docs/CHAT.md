# Chat Workspace

The chat command provides an interactive terminal client with persistent sessions stored on disk. See `docs/cli.md` for how `<data_home>` is resolved.

## Commands

### `hugind chat`
Launches the chat wizard:
- Start a new chat by config name
- Resume an existing session

If you pass an argument, Hugind treats it as either a session ID (if it exists) or a config name (if it does not) and starts a chat.

### `hugind chat start <config>`
Creates a new session using the specified config name and enters chat.

### `hugind chat resume <session-id>`
Loads an existing session and enters chat.

### `hugind chat list`
Lists session IDs and last active time.

### `hugind chat <config_or_session>`
Directly start a chat by config name or resume a session by ID.

### `hugind chat delete`
Interactive session deletion.

## Session Storage

Sessions are stored as JSON under `<data_home>/chats/` and include:
- `model` (the config name)
- `messages` (chat history)
- `last_active` timestamp
- `title` (optional, derived from the first user message)

## Slash Commands

- `/help` shows available slash commands.
- `/image <path>` attaches an image to the next message.
- `/sys <path>` injects a system prompt from a text file.
- `/clear` clears the terminal screen.
- `/exit` or `/quit` ends the session loop.

On Ctrl+C, Hugind sends a hibernate request so server state can persist.

## Best Practices

- Keep session IDs if you want to resume long-running conversations.
- If you see a context error, restart the chat to rehydrate history.
- Use `hugind server start <config>` before chatting if the server is not running.
