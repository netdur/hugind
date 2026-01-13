# Agent Development Guide

This guide explains how to build and run Hugind agents locally, including the
manifest format, permissions, and available capabilities.

## Quick Start

1. Create a directory with `agent.yaml` and an entry point (default `main.dart`).
2. Start a server config (the agent calls the local server).
3. Run the agent by path for fast iteration.

Example:

```bash
hugind server start metal_unified
hugind agent run ./my-agent
```

## Agent Layout

```
my-agent/
  agent.yaml
  main.dart
```

The entry point can be any filename you set in `agent.yaml` (for example,
`main.drt` in the examples).

## Entry Point API

Your entry point should export a `main` function that receives a context map.
The context includes arguments and capability objects.

```dart
dynamic main(Map<String, dynamic> context) async {
  var sys = context['capabilities']['sys'];
  var llm = context['capabilities']['llm'];
  var net = context['capabilities']['net'];

  sys.print('Hello from agent');
  var answer = await llm.chat('Give me a short checklist for a release.');
  sys.print(answer);
}
```

Available keys:

- `context['args']` list of CLI args passed after the agent name/path.
- `context['capabilities']['sys']` local IO helpers (print, confirm, readInput, run).
- `context['capabilities']['llm']` local server chat helper.
- `context['capabilities']['net']` HTTP fetch helper (domain allowlist).

## Manifest (`agent.yaml`)

Minimal example:

```yaml
name: "example-agent"
version: "0.1.0"
description: "Demo agent"
entry_point: "main.dart"
backend: "metal_unified"
```

Notes:

- `name` must be alphanumeric and may include `-` or `_`.
- `entry_point` defaults to `main.dart` if omitted.
- `backend` is the server config name; default is `metal_unified`.

## Permissions

Permissions are optional but recommended. The runtime enforces these:

- `permissions.filesystem.allowed_paths` limits file access.
- `permissions.network.allowed_domains` limits outbound HTTP domains.
- `permissions.shell.allow` enables `sys.run(...)`.

Example:

```yaml
permissions:
  filesystem:
    allowed_paths:
      - "/Users/me/projects"
  network:
    allowed_domains:
      - "api.example.com"
  shell:
    allow: false
```

Notes:

- The agent directory is always allowed by default.
- If you run an installed agent and pass a directory as the first argument,
  that directory is also allowed for file access.
- Keys like `filesystem.read` and `filesystem.write` are informational only.

## MCP Tools

Agents can call MCP tools via `sys.tools.list()` and `sys.tools.call(...)`.
You must configure MCP servers in `<data_home>/settings.yml` (see `docs/cli.md`):

```yaml
mcp_servers:
  filesystem:
    command: "npx"
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/Users/me/projects"]
```

Then in your agent:

```dart
var sys = context['capabilities']['sys'];
var tools = await sys.tools.list();
var result = await sys.tools.call('filesystem_read_file', {'path': 'README.md'});
```

## Running and Iterating

Local dev (fastest loop):

```bash
hugind agent run ./my-agent
```

Install to the Hugind agent directory:

```bash
hugind agent install ./my-agent
hugind agent run example-agent
```

Make sure the server is running with the configured backend:

```bash
hugind server start metal_unified
```
