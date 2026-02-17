# Hugind Agent Command Manual

## NAME

`hugind agent` - run and manage agents.

## SYNOPSIS

`hugind agent <subcommand> [options]`

## DESCRIPTION

The agent command runs agent definitions or workflows and supports listing,
installing, and removing agents.

## SUBCOMMANDS

### `hugind agent run <path> [--cwd <path>] [-- <args...>]`

Runs an agent or workflow from a file path, passing any additional arguments
through to the agent runtime.

Options:
- `--cwd <path>`: override runtime working directory and host filesystem root
  for this run. JS/WASM module loading still stays scoped to the agent folder.
  If `--cwd` points outside the agent root, set
  `permissions.filesystem.allow_outside_agent_root: true` in `agent.yaml`.

### `hugind agent list`

Lists agents installed under `~/.hugind/agents`.

### `hugind agent install <path>`

Installs an agent from a local folder (or `agent.yaml`) or from a web URL.
The installer reads `agent.yaml`, prints requested permissions, and asks for
confirmation before copying the agent into `~/.hugind/agents/<agent-name>`.

Accepted inputs:
- Local folder containing `agent.yaml`
- Direct path to `agent.yaml`
- Local `.zip` containing a single agent
- Web URL pointing at a folder or `agent.yaml`
- Web URL pointing at a `.zip` containing a single agent

Examples:
```bash
hugind agent install /path/to/agent-folder
hugind agent install /path/to/agent.yaml
hugind agent install /path/to/agent.zip
hugind agent install https://example.com/agents/my-agent/
hugind agent install https://example.com/agents/my-agent/agent.yaml
hugind agent install https://example.com/agents/my-agent.zip
```

Notes:
- If the agent already exists, the installer will ask before overwriting.
- For web installs, only `agent.yaml` and the `entry_point` are downloaded unless a `.zip` is used.

### `hugind agent remove`

Removes an installed agent from `~/.hugind/agents/<agent-name>`.

## HELP

Run `hugind agent --help` to see flags and options.
