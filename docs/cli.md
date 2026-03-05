# Hugind CLI Manual

## NAME

`hugind` - command-line interface for agents, configs, models, chats, the server, and the stdio bridge.

## SYNOPSIS

`hugind <command> [options]`

Common chat shorthand:

`hugind chat [start|resume|list|delete|<session-id-or-config>]`

## DESCRIPTION

`hugind` provides a small set of top-level commands for managing agents,
configurations, models, chat sessions, and a server. If a command fails,
`hugind` prints an error and exits with code `1`.

## COMMANDS

Top-level subcommands:

- `hugind agent ...`
- `hugind config ...`
- `hugind model ...`
- `hugind chat ...`
- `hugind server ...`
- `hugind stdio`

### `hugind agent`

See `docs/cli_agent.md`.

### `hugind config`

See `docs/cli_config.md`.

### `hugind model`

See `docs/cli_model.md`.

### `hugind chat`

See `docs/cli_chat.md`.

Behavior note:

- Running `hugind chat` with no subcommand opens the interactive chat wizard.
- Running `hugind chat <value>` treats `<value>` as a session id if it exists;
  otherwise it starts a new chat using `<value>` as config/model target.

### `hugind server`

See `docs/cli_server.md`.

### `hugind stdio`

See `docs/stdio_bridge.md`.

## HELP

Run `hugind --help` or `hugind <command> --help` to see all flags and options.
