# Hugind CLI Manual

## NAME

`hugind` - command-line interface for agents, configs, models, the server, and the stdio bridge.

## SYNOPSIS

`hugind <command> [options]`

## DESCRIPTION

`hugind` provides a small set of top-level commands for managing agents,
configurations, models, and a server. If a command fails,
`hugind` prints an error and exits with code `1`.

## COMMANDS

- `hugind agent ...` -- run, install, list, remove, team
- `hugind config ...` -- list, validate, init, defaults, info, remove
- `hugind model ...` -- add, show, list, remove, migrate
- `hugind server ...` -- start, list, stop
- `hugind stdio` -- stdio bridge for desktop/app integration

### `hugind agent`

See `docs/cli_agent.md`.

### `hugind config`

See `docs/cli_config.md`.

### `hugind model`

See `docs/cli_model.md`.

### `hugind server`

See `docs/cli_server.md`.

### `hugind stdio`

See `docs/stdio_bridge.md`.

## ENVIRONMENT

- `HUGIND_TRACE=1` -- enable trace output for agentic execution

## HELP

Run `hugind --help` or `hugind <command> --help` to see all flags and options.
