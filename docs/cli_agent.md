# Hugind Agent Command Manual

## NAME

`hugind agent` - run and manage agents.

## SYNOPSIS

`hugind agent <subcommand> [options]`

## DESCRIPTION

The agent command runs agent definitions or workflows and supports listing,
installing, and removing agents.

## SUBCOMMANDS

### `hugind agent run <path> [--cwd <path>] [--log-file <path>] [-- <args...>]`

Runs an agent or workflow from a file path, passing any additional arguments
through to the agent runtime.

`<path>` can be:
- a local agent directory (contains `agent.yaml`)
- a direct local entry path
- a local workflow `.yaml` file
- an installed agent name (resolved from the agents install directory)

Options:
- `--cwd <path>`: override runtime working directory and host filesystem root
  for this run. JS/WASM module loading still stays scoped to the agent folder.
  If `--cwd` points outside the agent root, set
  `permissions.filesystem.allow_outside_agent_root: true` in `agent.yaml`.
- `--log-file <path>`: write runtime audit logs for this run to an explicit file
  path. Parent directories are created if needed. The file is opened in append
  mode.

### `hugind agent list`

Lists agents installed under `config_home()/agents`
(`$XDG_CONFIG_HOME/hugind/agents` or `~/.hugind/agents` on Unix-like systems).

### `hugind agent install <path>`

Installs an agent from a local folder (or `agent.yaml`) or from a web URL.
The installer reads `agent.yaml`, prints requested permissions, and asks for
confirmation before copying the agent into the agents install directory.

Accepted inputs:
- Local folder containing `agent.yaml`
- Direct path to `agent.yaml`
- Local `.zip` containing a single agent
- Web URL pointing at a folder or `agent.yaml`
- Web URL pointing at a `.zip` containing a single agent
- GitHub `github.com/<owner>/<repo>/tree/...` and `.../blob/...` URLs
  (auto-resolved to `raw.githubusercontent.com`)

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

`hugind agent remove <name>`

Removes an installed agent from the agents install directory.

## HELP

Run `hugind agent --help` to see flags and options.
