# Hugind Model Command Manual

## NAME

`hugind model` - manage local model repositories and GGUF files.

## SYNOPSIS

`hugind model <subcommand> [options]`

## DESCRIPTION

The model subcommands manage local Hugging Face repositories and GGUF model
files. You can list what is already downloaded, add models from a remote repo,
inspect local files, and remove files or entire repositories.

Local storage layout: `~/.hugind/models/<user>/<repo>/`.

## SUBCOMMANDS

### `hugind model list`

Lists all downloaded model repositories.

### `hugind model add [repo] [-y]`

Downloads GGUF files from a Hugging Face repository. If `repo` is omitted, you
are prompted for one. You then select one or more files to download.

Features:
- Download resume (interrupted downloads continue from where they left off)
- SHA256 integrity verification
- Concurrent downloads (up to 2 at a time)
- Skips files that are already downloaded with correct size

Options:
- `-y, --yes`: skip confirmation prompts (download all files)

### `hugind model show <repo>`

Lists local files and sizes for a repository.

### `hugind model remove [repo] [-y]`

Deletes a repository or specific files. If `repo` is omitted, you are prompted
to choose. You can delete the entire repo or select specific files.

Options:
- `-y, --yes`: skip confirmation prompts

### `hugind model migrate`

Migrates models from the old storage layout (`~/.hugind/<user>/<repo>/`) to the
new layout (`~/.hugind/models/<user>/<repo>/`). Also updates model paths in
config files under `~/.hugind/configs/`.

This runs automatically on first startup after upgrading. Can also be run
manually.

## HELP

Run `hugind model --help` to see flags and options.
