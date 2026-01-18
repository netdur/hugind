# Agent Builder

The agent builder helps you scaffold or update a local agent by asking for a short description and then generating `agent.yaml` and `main.dart`.

## Quick Start

Start the server and run the builder agent against a target folder:

```bash
hugind server start metal_unified
hugind agent run ./examples/agents/builder dev ./examples/agents/my-agent
```

The builder will prompt for a description and then write:

- `./examples/agents/my-agent/agent.yaml`
- `./examples/agents/my-agent/main.dart`

## Notes

- Re-run the builder to update an existing agent.
- The target directory must be writable.
- Review the generated code and permissions before use.
