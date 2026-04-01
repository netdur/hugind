# Hugind Config Command Manual

## NAME

`hugind config` - manage configuration files and defaults.

## SYNOPSIS

`hugind config <subcommand> [options]`

## DESCRIPTION

The config subcommands manage configuration files and global defaults. You can
list and validate configs, inspect system hardware, set defaults, and create a
new config using an interactive wizard.

Config files are stored in `~/.hugind/configs/`.
Global settings are stored in `~/.hugind/settings.yml`.

## SUBCOMMANDS

### `hugind config list`

Lists all saved configs. Only `.yml` and `.yaml` files are shown.

### `hugind config validate [path]`

Validates a config file. Defaults to `config.yaml` in the current directory.

### `hugind config info`

Prints system information: OS, CPU, memory, disk, GPU.

### `hugind config init <name> [--model <path>]`

Interactive config generator:

1. Probes system hardware (CPU, RAM, GPU).
2. Prompts for a model:
   - If `--model` is passed, that file path is used.
   - Otherwise scans downloaded model repositories for `.gguf` files.
3. Auto-detects vision projector files (mmproj) in the same folder.
4. Asks whether to enable auto-fit or choose a context size manually.
5. Auto-configures GPU layers, threads, flash attention, and KV offload
   based on detected hardware.
6. Writes the config to `~/.hugind/configs/<name>.yml`.

### `hugind config remove <name>`

Deletes a config file after confirmation.

### `hugind config defaults [--hf-token <token>] [--set key=value]`

Shows or updates global defaults:

- `--hf-token <token>`: set Hugging Face API token
- `--set key=value`: set an arbitrary key-value pair (can be repeated)

Without arguments, prints current defaults (sensitive values are masked).

## HELP

Run `hugind config --help` to see flags and options.
