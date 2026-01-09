# Developer Guide

This document describes how Hugind is structured, where key logic lives, and how to extend it.

## Repository Structure

- CLI entrypoint: `bin/hugind.dart`
- CLI commands: `lib/commands/*.dart`
- Server bootstrap + HTTP routes: `lib/server/bootstrap.dart`
- Config loader: `lib/server/config/config_loader.dart`
- Engine manager and runtime: `lib/server/engine/`
- Agent sandbox and capabilities: `lib/agent/`
- Model download and storage: `lib/repo_manager.dart`
- Global settings: `lib/global_settings.dart`

## Command Dispatch

`bin/hugind.dart` registers top-level commands:
- `model`, `config`, `server`, `agent`, `chat`

Each command is a `Command` subclass with subcommands defined in:
- `lib/commands/model_command.dart`
- `lib/commands/config_command.dart`
- `lib/commands/server_command.dart`
- `lib/commands/agent_command.dart`
- `lib/commands/chat_command.dart`

To add a new subcommand:
1. Create a new `Command` subclass in the appropriate file.
2. Register it in the parent command constructor.
3. Keep argument parsing in the subcommand to avoid cross-command coupling.

## Config Loading Pipeline

`ConfigLoader.load()` merges:
- `server` settings (host, port, API key, concurrency)
- `model` settings (path, GPU layers, mmap, mmproj)
- `context` settings (n_ctx, batch sizes, threads, cache types)
- `sampling` defaults
- `chat.format` if set

Important behaviors:
- Model paths are resolved and validated.
- `server.library_path` is resolved if present; otherwise runtime auto-detection is used.
- `server.max_slots` becomes `ContextParams.nSeqMax` for concurrency.

## Server Lifecycle

`bootstrapServer()`:
- Checks port availability
- Deploys model via `EngineManager`
- Registers HTTP routes (`/health`, `/v1/chat/completions`, `/v1/embeddings`, etc.)
- Enables API key auth middleware when configured

The server runs in the foreground and shuts down on Ctrl+C, releasing model resources.

## Sessions and State

- Chat sessions use `X-Session-ID` headers.
- The engine uses a session pool for stateless requests (`stateless-0..31`).
- `POST /v1/chat/hibernate` triggers session hibernation.

On the CLI side, chat history is stored in `~/.hugind/chats/*.json` and rehydrated on resume.

## Agent Runtime

Agents run through `dart_eval` with a bridge layer in `lib/agent/sandbox.dart`.

Capabilities are implemented in `lib/agent/capabilities.dart` and enforced by:
- Path allowlists for filesystem access
- Domain allowlists with SSRF protections for network access
- Optional shell execution
- MCP client support for tool execution

## Extending the API

To add a new endpoint:
1. Implement a handler under `lib/server/api/`.
2. Register it in `lib/server/bootstrap.dart`.
3. Update `docs/api.md` with the new behavior.

## Best Practices

- Keep CLI commands small and focused; move shared logic to services.
- Avoid blocking operations in HTTP handlers; prefer isolate-backed engine work.
- Preserve config backward compatibility by defaulting missing fields.
- Document any new settings in `docs/config.md` and `docs/api.md`.
