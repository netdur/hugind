# CLI Overview

## Global Usage

```
hugind [--version] <command> [subcommand] [options]
```

- `--version` prints the binary version and exits.
- `hugind --help` shows the top-level command list.
- `hugind <command> --help` shows subcommand help provided by the Dart args runner.

## Command Index

- `hugind model ...` manage local models (download, list, remove)
- `hugind config ...` hardware probe and config utilities
- `hugind server ...` run and manage inference servers
- `hugind agent ...` install/run sandboxed agents
- `hugind chat ...` interactive terminal workspace

## Hugind Home Directories

Hugind stores files under a per-user home directory.

- macOS/Linux: `~/.hugind` unless `XDG_CONFIG_HOME` is set, then `$XDG_CONFIG_HOME/hugind`
- Windows: `%APPDATA%\hugind` or `%USERPROFILE%\.hugind`

Common paths:

- Configs: `<home>/configs/*.yml`
- Models: `<home>/<hf_user>/<hf_repo>/*.gguf`
- Agents: `<home>/agents/<agent_name>/`
- Chat sessions: `<home>/chats/*.json`
- Global settings: `<home>/settings.yml`

## Quick Examples

```
# List downloaded models
hugind model list

# Create a new config
hugind config init my-assistant

# Start server
hugind server start my-assistant

# Start chat wizard
hugind chat
```
