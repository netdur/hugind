# Hugind Server Command Manual

## NAME

`hugind server` - start, list, and stop the local server.

## SYNOPSIS

`hugind server <subcommand> [options]`

## DESCRIPTION

The server command starts a local server for a given config, reports status for
known configs, and stops a running server on the configured port.

## SUBCOMMANDS

### `hugind server start <config> [--port <port>]`

Starts a server using a named config from the configs directory. If `--port` is
provided, it overrides the port in the config. The server loads the model from
the config and starts the engine loop, then listens on `<host>:<port>`.

### `hugind server list`

Lists saved configs and their status. For each config, it reads `server.host`
and `server.port` from the config (defaulting to `127.0.0.1:8080`) and checks
`/v1/monitor` to determine if the server is up. The output is `up` or `down`.

### `hugind server stop <config>`

Stops a server by looking up its configured host/port, checking health, and
terminating processes listening on the port. On Windows, stop is not
implemented and will print a notice.

## NOTES

1. `server stop` uses `lsof` and `kill -9` on non-Windows systems.
2. If the config cannot be found, the command returns an error.

## HELP

Run `hugind server --help` to see flags and options.
