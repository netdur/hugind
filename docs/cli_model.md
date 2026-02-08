# Hugind Model Command Manual

## NAME

`hugind model` - manage local model repositories and GGUF files.

## SYNOPSIS

`hugind model <subcommand> [options]`

## DESCRIPTION

The model subcommands manage local Hugging Face repositories and GGUF model
files. You can list what is already downloaded, add models from a remote repo,
inspect local files, and remove files or entire repositories.

## SUBCOMMANDS

### `hugind model list`

Prints all downloaded model repositories. If none exist, it prints a message
suggesting `hugind model add`.

### `hugind model add [repo]`

Downloads GGUF files from a Hugging Face repository. If `repo` is omitted, you
are prompted for one. You then select one or more GGUF files to download.

### `hugind model show <repo>`

Lists files and sizes for a local repository. If the repo is not present
locally, it reports that it cannot be found.

### `hugind model remove [repo]`

Deletes a repository or specific files. If `repo` is omitted, you are prompted
to choose a local repository. You can delete the entire repo, or select specific
files. If a repo becomes empty, `hugind` offers to delete the folder.

## HELP

Run `hugind model --help` to see flags and options.
