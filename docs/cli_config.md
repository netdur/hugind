# Hugind Config Command Manual

## NAME

`hugind config` - manage configuration files and defaults.

## SYNOPSIS

`hugind config <subcommand> [options]`

## DESCRIPTION

The config subcommands manage configuration files stored under the Hugind data
directory. You can list and validate configs, inspect system hardware, set
global defaults, and create a new config using an interactive wizard.

## SUBCOMMANDS

### `hugind config list`

Lists all saved configs in the configs directory. Only `.yml` and `.yaml` files
are shown. If none exist, it prints `No configs found.`.

### `hugind config validate [path]`

Validates a config file. If `path` is omitted, it defaults to `config.yaml` in
the current working directory. On success it prints `Configuration is valid.`
On failure it prints the validation error and exits with an error.

### `hugind config info`

Prints system information (OS, CPU, memory, disk, GPU) and a recommended
hardware preset for configuration.

### `hugind config init <name> [--model <path>]`

Interactive config generator. It:

1. Probes system hardware and recommends a preset.
2. Prompts for a hardware preset (`metal_unified`, `cuda_dedicated`, `cpu_only`).
3. Prompts for a model:
   - If you pass `--model`, that file path is used.
   - Otherwise it scans local model repositories and prompts for a `.gguf`.
   - If no repositories exist, it prompts for an absolute `.gguf` path.
4. Auto-detects a vision projector file (e.g. `mmproj`) in the same folder.
5. Prompts for context size.
6. Writes the config to the configs directory as `<name>.yml`.

If a config already exists, it prompts before overwriting.

### `hugind config remove <name>`

Deletes a config file from the configs directory after confirmation.

### `hugind config defaults [--hf-token <token>]`

Shows or updates global defaults. With no arguments, it prints the current
defaults and usage help. With arguments, it updates:

- `--hf-token` sets the Hugging Face token (stored as `hf_token`).

## HELP

Run `hugind config --help` to see flags and options.
