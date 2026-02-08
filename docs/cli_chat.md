# Hugind Chat Command Manual

## NAME

`hugind chat` - start, resume, list, and delete chat sessions.

## SYNOPSIS

`hugind chat [subcommand] [options]`

## DESCRIPTION

The chat command manages interactive chat sessions backed by a configured
model. You can start a new session, resume an existing one, list sessions, or
delete a session. If you run `hugind chat` without a subcommand, it launches
the interactive wizard.

## SUBCOMMANDS

### `hugind chat start [config]`

Starts a new chat session using the given config name. If no config is
provided, it prompts you to select a config or enter a name manually.

### `hugind chat resume [id]`

Resumes an existing session by id. If no id is provided, you are prompted to
choose from existing sessions. If no sessions exist, it offers to start a new
chat instead.

### `hugind chat list`

Lists sessions with their id, last active time, and title.

### `hugind chat delete [id]`

Deletes a session after confirmation. If no id is provided, you are prompted to
choose from existing sessions.

### `hugind chat` (no subcommand)

Launches the interactive wizard with options:

1. Start New Chat
2. Resume Chat
3. List Sessions
4. Delete Session
5. Exit

### `hugind chat <arg>` (default)

If you pass a single positional argument that does not match a subcommand,
`hugind` treats it as:

1. A session id if it exists, and resumes it.
2. Otherwise, a config name and starts a new session.

## INTERACTIVE COMMANDS

Within a chat session, the interactive prompt supports:

1. `/help` - show available commands.
2. `/image <path>` - attach an image (PNG or JPEG).
3. `/text <path>` - attach a text file.
4. `/clear` - clear the screen.
5. `/exit` or `/quit` - exit the session.

## SESSION BEHAVIOR

1. The session prints recent context if prior messages exist.
2. Responses are streamed to the terminal.
3. The session is saved after each exchange.
4. The title is auto-generated after the first assistant reply.

## HELP

Run `hugind chat --help` to see flags and options.
