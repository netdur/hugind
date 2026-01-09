# Model Management

Hugind manages GGUF model files downloaded from Hugging Face. Models are stored under the Hugind home directory in `~/.hugind/<user>/<repo>/`.

## Commands

### `hugind model list`
Lists all locally downloaded model repositories.

### `hugind model add <user/repo>`
Interactive download of `.gguf` files from Hugging Face.

What it does:
- Fetches the repo metadata via the Hugging Face API.
- Filters for `.gguf` files.
- Presents a multi-select UI.
- Downloads each selected file with progress reporting.

Example:
```
hugind model add TheBloke/Llama-2-7B-Chat-GGUF
```

### `hugind model show <user/repo>`
Lists local `.gguf` files and their sizes within a repo.

### `hugind model remove <user/repo>`
Interactive deletion of files or entire repositories.

Behavior:
- If the repo is empty, it offers to delete the folder.
- If files exist, it lets you delete individual files or the whole repo.
- Cleans up empty directories afterward.

## Hugging Face Authentication

If a repo is gated or private, set a token:

```
hugind config defaults --hf-token <hf_token>
```

This stores the token in `~/.hugind/settings.yml` and is used for downloads.

## Best Practices

- Prefer `.gguf` builds that match your hardware (Metal/CUDA/CPU) and desired quantization.
- Keep disk headroom: a single 7B model can be multiple GBs.
- Use `hugind model show` before deleting to avoid removing the wrong file.
- For multi-model testing, name configs after the repo+quant to keep them distinct.
