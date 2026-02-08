# Hugind CLI Manual

## NAME

`hugind` - command-line interface for agents, configs, models, chats, and the server.

## SYNOPSIS

`hugind <command> [options]`

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

### `hugind agent`

See `docs/cli_agent.md`.

### `hugind config`

See `docs/cli_config.md`.

### `hugind model`

See `docs/cli_model.md`.

### `hugind chat`

See `docs/cli_chat.md`.

### `hugind server`

See `docs/cli_server.md`.

## HELP

Run `hugind --help` or `hugind <command> --help` to see all flags and options.
