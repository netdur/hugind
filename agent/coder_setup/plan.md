# coder_setup plan

`coder_setup` is a deterministic Hugind agent that manages per-run git worktrees for swarm agents.
It does **not** use the LLM.

It supports `--mode build|cleanup`.

## CLI contract

Example:

```bash
./target/release/hugind agent run agent/coder_setup -- \
  --repo <path/to/repo> \
  --run-id <id> \
  --mode build \
  --worktrees-dir <path/to/repo>/.worktrees \
  --base-ref <branch-or-sha> \
  --branch-prefix agent/
```

Minimal required:

* `--repo`
* `--run-id`

Optional:

* `--worktrees-dir` (default: `<repo>/.worktrees`)
* `--base-ref` (default: current branch of `<repo>`)
* `--branch-prefix` (default: `agent/`)
* `--mode` (default: `build`; alternative: `cleanup`)
* `--reuse` (default: `true`)

  * if `true` and the worktree already exists at the expected path, return it
  * if `false` and the worktree already exists, fail
* `--delete-branch` (default: `false`, used only in `cleanup`)
* `--force` (default: `false`, used only in `cleanup`)

## What it does (build mode)

1. Validate

* `--repo` exists and is a directory
* `git -C <repo> rev-parse --is-inside-work-tree` succeeds
* sanitize `run-id` into a safe directory/branch suffix:

  * allow only `A-Za-z0-9._-`
  * fail if empty after sanitization

2. Compute

* `branch = <branch-prefix><run-id>` (example: `agent/898b0878-fe90-46c3-a041-b49c48434231`)
* `worktree_path = <worktrees-dir>/<run-id>`

3. Discover existing state

* `git -C <repo> worktree list --porcelain`

Rules:

* If `worktree_path` already exists:

  * if `--reuse=true`, return it after verifying:

    * it is a git worktree
    * it is on `branch` (or record the current branch clearly in output)
  * if `--reuse=false`, fail with a clear message
* If `branch` is already checked out in another worktree, fail clearly (git also enforces this)

4. Create the worktree

* Ensure `<worktrees-dir>` exists (`mkdir -p`)
* Determine `base_ref`:

  * if `--base-ref` provided, use it
  * else use `git -C <repo> rev-parse --abbrev-ref HEAD`

Create:

* preferred:

  * `git -C <repo> worktree add -b <branch> <worktree_path> <base_ref>`
* if branch already exists:

  * `git -C <repo> worktree add <worktree_path> <branch>`

5. Return result via `set_result`

* `status`: `success` | `failed`
* `repo`
* `run_id`
* `mode`
* `worktree_path`
* `branch`
* `base_ref`
* `errors`: string[]
* `audit`:

  * `commands_run`: string[] (or `{ cmd, args }[]`)

## What it does (cleanup mode)

1. Validate repo

* same repo checks as build mode

2. Compute

* `branch = <branch-prefix><run-id>`
* `worktree_path = <worktrees-dir>/<run-id>`

3. Remove worktree

* `git -C <repo> worktree remove <worktree_path>`
* if `--force=true`, use:

  * `git -C <repo> worktree remove --force <worktree_path>`

4. Prune stale metadata

* `git -C <repo> worktree prune`

5. Optionally delete branch

* only if `--delete-branch=true`:

  * `git -C <repo> branch -D <branch>` (or `-d` if you want safer behavior)

## Guardrails

* Never call a shell; only `spawn("git", [...])` / argument-array execution
* Refuse `--worktrees-dir` outside `--repo` unless explicitly allowed (recommended: refuse)
* Never delete anything unless `--mode cleanup`
* Always log commands run (and surface them in `audit.commands_run`)

## How it plugs into the swarm

Host flow:

1. `coder_setup --mode build` → returns `worktree_path`
2. Run:

   * `coder` (generates `output.diff`)
   * `coder_audit` (produces pass/fail and `issue.md` if needed)
   * `coder_docs` (writes `docs.md`)
   * `patcher` (applies patch + runs checks)
     all with `--cwd <worktree_path>`
3. Optional: `coder_setup --mode cleanup` when finished



# 1) Build phase
./target/release/hugind agent run agent/coder_setup -- \
  --repo /Users/adel/Workspace/atlas_workspace/test \
  --run-id trial-001 \
  --mode build

# 2) Build phase with explicit options (optional)
./target/release/hugind agent run agent/coder_setup -- \
  --repo /Users/adel/Workspace/atlas_workspace/test \
  --run-id trial-001 \
  --mode build \
  --worktrees-dir /Users/adel/Workspace/atlas_workspace/test/.worktrees \
  --branch-prefix agent/ \
  --reuse true

# 3) Verify worktree exists
git -C /Users/adel/Workspace/atlas_workspace/test worktree list --porcelain

# 4) Cleanup phase (when done)
./target/release/hugind agent run agent/coder_setup -- \
  --repo /Users/adel/Workspace/atlas_workspace/test \
  --run-id trial-001 \
  --mode cleanup

# 5) Cleanup + delete branch (optional)
./target/release/hugind agent run agent/coder_setup -- \
  --repo /Users/adel/Workspace/atlas_workspace/test \
  --run-id trial-001 \
  --mode cleanup \
  --delete-branch true \
  --force true
