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

Hugind uses two roots today: one for configs/agents, and one for data.

**Config home (configs, agents):**
- macOS/Linux: `$XDG_CONFIG_HOME/hugind` if set, otherwise `~/.hugind`
- Windows: `%APPDATA%\hugind` if set, otherwise `%USERPROFILE%\.hugind`

**Data home (models, chats, sessions, settings):**
- macOS/Linux: `~/.hugind`
- Windows: `%USERPROFILE%\.hugind`

Common paths:

- Configs: `<config_home>/configs/*.yml`
- Agents: `<config_home>/agents/<agent_name>/`
- Models: `<data_home>/<hf_user>/<hf_repo>/*.gguf`
- Chat sessions: `<data_home>/chats/*.json`
- Server sessions: `<data_home>/sessions/`
- Global settings: `<data_home>/settings.yml`

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
