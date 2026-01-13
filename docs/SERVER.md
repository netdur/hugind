# Server Runtime

The server hosts an OpenAI-compatible API over HTTP, backed by `llama_cpp_dart`. See `docs/cli.md` for how `<config_home>` is resolved.

## Commands

### `hugind server`
Launches an interactive wizard to select a config and start a server.

### `hugind server list`
Lists all configs under `<config_home>/configs` and checks if their `/health` endpoint is reachable.

### `hugind server start <config_name>`
Starts a server in the foreground using `<config_home>/configs/<config_name>.yml`.

Options:
- `-p, --port <port>` override the configured port
- `--lib <path>` override the shared library path

Behavior:
- Loads the config and resolves the model path.
- Determines the `libllama` path (CLI `--lib` > config `library_path` > auto-detect).
- Bootstraps the server and prints health and API URLs.

### `hugind server stop <config_name>`
Prints OS-specific commands to stop the running server for a config and checks health.

## Health and URLs

When running, the server prints:

- Health: `http://127.0.0.1:<port>/health`
- API base: `http://127.0.0.1:<port>/v1`

## API Key Auth

If `server.api_key` is set, requests must include `Authorization: Bearer <token>`.
The `/health` endpoint is not authenticated.

## Embeddings-Only Mode

If `server.embeddings` is `true`, only `/v1/embeddings` is enabled. Chat and completions endpoints are disabled.

## Best Practices

- Use `hugind server list` to confirm configs and status before starting.
- For local development, keep `host: 127.0.0.1` and use an API key if the port is exposed.
- Start the server in a dedicated terminal to keep Ctrl+C available for shutdown.
