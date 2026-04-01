# Hugind Agent Command Manual

## NAME

`hugind agent` - run and manage agents.

## SYNOPSIS

`hugind agent <subcommand> [options]`

## DESCRIPTION

The agent command runs agent definitions or workflows, supports listing,
installing, and removing agents, and can orchestrate multi-agent teams.

## SUBCOMMANDS

### `hugind agent run <path> [--cwd <path>] [--log-file <path>] [-- <args...>]`

Runs an agent or workflow from a file path, passing any additional arguments
through to the agent runtime.

`<path>` can be:
- a local agent directory (contains `agent.yaml`)
- a direct local entry path
- a local workflow `.yaml` file
- an installed agent name (resolved from the agents install directory)

If the agent has `mode: agentic` in its `agent.yaml`, the runtime drives an
LLM tool-use loop automatically. Pass the task via `--prompt`:
```bash
hugind agent run agent/ma-reader --prompt "Find all Python files in this project"
```

Options:
- `--cwd <path>`: override runtime working directory and host filesystem root
  for this run. JS/WASM module loading still stays scoped to the agent folder.
  If `--cwd` points outside the agent root, set
  `permissions.filesystem.allow_outside_agent_root: true` in `agent.yaml`.
- `--log-file <path>`: write runtime audit logs to an explicit file path.

### `hugind agent team <goal> --agents <paths> [--backend <config>] [--concurrency <n>]`

Orchestrates a multi-agent team to accomplish a goal. A coordinator agent
decomposes the goal into tasks, assigns them to team members, and synthesizes
the final result.

```bash
hugind agent team "Build a REST API for user management" \
  --agents agent/ma-architect,agent/ma-developer,agent/ma-tester,agent/ma-reviewer \
  --backend qwen-32b
```

Options:
- `--agents <paths>`: comma-separated list of agent directories
- `--backend <config>`: config name for the coordinator LLM (default: uses default server)
- `--concurrency <n>`: max concurrent agents (default: 4)

The team command:
1. Sends the goal to the coordinator LLM for task decomposition
2. Parses the returned JSON task array with dependencies
3. Executes tasks in parallel (respecting dependency order)
4. Each agent gets shared memory, messaging, and the user's working directory
5. Synthesizes a final summary after all tasks complete

All agents share a fresh session on the LLM server for KV cache reuse.

### `hugind agent list`

Lists agents installed under `~/.hugind/agents`.

### `hugind agent install <path>`

Installs an agent from a local folder, URL, or zip file.
Prints requested permissions and asks for confirmation.

Accepted inputs:
- Local folder containing `agent.yaml`
- Direct path to `agent.yaml`
- Local `.zip` containing a single agent
- Web URL pointing at a folder, `agent.yaml`, or `.zip`
- GitHub `github.com/<owner>/<repo>/tree/...` URLs

### `hugind agent remove <name>`

Removes an installed agent.

## ENVIRONMENT

- `HUGIND_TRACE=1`: enable detailed trace output for agentic execution

## HELP

Run `hugind agent --help` to see flags and options.
