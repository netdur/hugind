# Hugind Documentation

Welcome to the new Hugind docs. This set focuses on practical usage, clear CLI references, and developer-facing internals.

## Start Here

1. Download a model: `hugind model add <user/repo>`
2. Create a config: `hugind config init <name>`
3. Start the server: `hugind server start <name>`
4. Chat locally: `hugind chat`

## Topics

- CLI overview and command index: `docs/cli.md`
- Model management (Hugging Face downloads): `docs/model.md`
- Configuration and presets: `docs/config.md`
- Server runtime and lifecycle: `docs/server.md`
- Chat workspace and sessions: `docs/chat.md`
- Agent runtime and security: `docs/agent.md`
- API reference (OpenAI-compatible endpoints): `docs/api.md`
- Developer internals and architecture: `docs/developer.md`

## Conventions Used

- All commands are shown as `hugind <command> <subcommand> [args]`.
- Config and data files live under your Hugind home directory. See `docs/cli.md` for OS-specific paths.
- Examples assume a local server at `http://127.0.0.1:8080`.
