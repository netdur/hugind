# Hugind Model Management

The `hugind` CLI provides a simple way to download and organize GGUF models directly from Hugging Face. Models are stored in your home directory structure.

## Usage

```bash
hugind model <subcommand> [arguments]
```

## Storage Structure
Models are stored in: `~/.hugind/<user>/<repo>/<filename>.gguf`

*Example:*
`~/.hugind/TheBloke/Llama-2-7B-Chat-GGUF/llama-2-7b-chat.Q4_K_M.gguf`

## Commands

### 1. `add` (Download)
Interactively select and download files from a Hugging Face repository.

```bash
hugind model add <user/repo>
```

**Example:**
```bash
hugind model add TheBloke/Mistral-7B-Instruct-v0.2-GGUF
```

**Process:**
1.  Scans the remote repository for `.gguf` files.
2.  Displays a list where you can select one or multiple files (using Spacebar).
3.  Downloads the selected files with a progress bar.

---

### 2. `list`
Displays all model repositories (folders) currently downloaded on your machine.

```bash
hugind model list
```

**Output:**
```text
Downloaded Repositories:
----------------------------------------
TheBloke/Mistral-7B-Instruct-v0.2-GGUF
google/gemma-2b-it-GGUF
```

---

### 3. `show`
Lists the specific files available inside a local repository.

```bash
hugind model show <user/repo>
```

**Example:**
```bash
hugind model show google/gemma-2b-it-GGUF
```

**Output:**
```text
Files in google/gemma-2b-it-GGUF:
----------------------------------------
gemma-2b-it.Q4_K_M.gguf  (1500.23 MB)
gemma-2b-it.Q8_0.gguf    (2800.50 MB)
```

---

### 4. `remove`
Delete specific files or entire repositories to free up disk space.

```bash
hugind model remove <user/repo>
```

**Process:**
1.  Lists the files currently in that repository.
2.  Includes a special option `[DELETE ENTIRE REPO]` at the top.
3.  Allows multi-selection to delete specific quantizations.
4.  If the folder becomes empty, it asks to clean up the folder automatically.
