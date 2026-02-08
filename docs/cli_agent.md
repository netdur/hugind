# Hugind Agent Command Manual

## NAME

`hugind agent` - run and manage agents.

## SYNOPSIS

`hugind agent <subcommand> [options]`

## DESCRIPTION

The agent command runs an agent definition or workflow and provides placeholders
for install/remove functionality.

## SUBCOMMANDS

### `hugind agent run <path> [-- <args...>]`

Runs an agent from a file path, passing any additional arguments through to the
agent runtime.

### `hugind agent install <path>`

Installs an agent from a local folder (or `agent.yaml`) or from a web URL.
The installer reads `agent.yaml`, prints requested permissions, and asks for
confirmation before copying the agent into `~/.hugind/agents/<agent-name>`.

Accepted inputs:
- Local folder containing `agent.yaml`
- Direct path to `agent.yaml`
- Web URL pointing at a folder or `agent.yaml`

Examples:
```bash
hugind agent install /path/to/agent-folder
hugind agent install /path/to/agent.yaml
hugind agent install https://example.com/agents/my-agent/
hugind agent install https://example.com/agents/my-agent/agent.yaml
```

Notes:
- If the agent already exists, the installer will ask before overwriting.
- For web installs, only `agent.yaml` and the `entry_point` are downloaded.

### `hugind agent remove`

Not implemented yet. Prints `Agent remove not implemented yet`.

## HELP

Run `hugind agent --help` to see flags and options.
