# Workflow Plan

## 1. Product Direction
- Agents are core app functions, not optional plug-ins.
- Keep the Agents screen, but remove workflow composition/editing.
- Workflows are preset in code and tightly integrated with Runs/Review.

## 2. Scope
- Keep role-based agent assignment + command templates in Settings.
- Remove workflow editor UI.
- Support preset workflows only:
  - workflow_1
  - workflow_2

## 3. Core Principle
- `coder_setup` must run first to create/resolve workspace path.
- `task.md` is created only after setup returns workspace location.
- `coder_setup` runs again at the end for cleanup.
- Review decision controls next workflow:
  - Approve -> `workflow_2`
  - Reject -> stop / optional cleanup policy

## 4. Execution State Model
- Add run state machine:
  - queued -> setup_done -> task_saved -> coding_done -> review_pending -> approved|rejected -> finalized
- Persist per project in `<project>/.atlas/runs.json`.
- Persist:
  - runId (same as task-id)
  - selected backlog item
  - preset workflow name
  - step statuses + logs + timestamps
  - review decision + reviewer note

## 5. Preset Workflows (Code-defined)
- `workflow_1`:
  1. `coder_setup` (build workspace)
  2. save `task.md` into workspace
  3. context
  4. code
  5. audit
  6. docs
  7. `coder_setup` (cleanup)
- `workflow_2`:
  1. `coder_setup` (build workspace)
  2. patch
  3. merge
  4. `coder_setup` (cleanup)

## 6. Settings Screen Ownership
- Move agent and command configuration into Settings screen.
- Settings screen owns:
  - fixed role assignments
  - strict agent-name validation per role
  - command template editing for preset workflows (host args / agent args)
  - optional agent metadata/details preview
- Remove workflow editing from Agents screen.
- Agents screen can be removed or reduced to read-only status if needed.

## 7. Runs Screen
- Purpose: execute and dry-run preset workflows.
- Inputs:
  - backlog task selector (TASK/SPIKE)
  - preset selection (`workflow_1` / `workflow_2`) as applicable
- Behavior:
  - `run-id == task-id`
  - dry run shows exact command plan in order
  - execute uses Settings-defined role assignments + command templates

## 8. Review Screen Integration
- Add “AI Run Reviews” section:
  - pending run summary
  - links/artifacts
  - Approve / Reject actions
- Approve triggers `workflow_2`.
- Reject stops (or optional cleanup based on policy).

## 9. Command/Path Rules
- Agent folder paths are full absolute paths.
- Tokens:
  - `<project>` = selected project root
  - `<task-id>` = selected backlog task id
  - `<run-id>` = same as `<task-id>`
- Task file:
  - saved inside workspace path returned by `coder_setup --mode build`
  - created only after successful setup build

## 10. Failure Handling
- If post-setup step fails:
  - mark run failed
  - expose retry/cleanup actions
- Cleanup workflow is idempotent and always available.

## 11. Implementation Order
1. Remove workflow editor UI from Agents screen.
2. Add preset workflow definitions in code (single source of truth).
3. Keep role assignment + command template persistence in `<project>/.atlas/agents.json`.
4. Add run state persistence in `<project>/.atlas/runs.json`.
5. Implement Runs preset dry run + execution wiring from Settings configuration.
6. Implement Review approve/reject transitions.

## 12. Open Decisions
- Target merge branch rule (`main`, `develop`, per-project setting).
- Whether audit failure should still create a review task.
- Whether reject should allow a “retry dev” shortcut.


# technical

before -- we can add those two options for every run
--cwd /Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002
--log-file /Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002/<agent_name>.txt

"agent/<agent>" should be full path as set in settings

./target/release/hugind agent run agent/coder_setup -- \
  --repo <project> \
  --run-id <task-id> \
  --mode build

this will create worktree like

<project>/.worktrees/<task-id>

create 
<project>/.worktrees/<task-id>/tasks/<task-id>/task.md
and put task content there

then create context, this is like a research to find out what files to edit

./target/release/hugind agent run agent/coder_context -- \
  --task <project>/.worktrees/<task-id>/tasks/<task-id>/task.md \
  --project <project>/.worktrees/<task-id> \
  --context <project>/.worktrees/<task-id>/tasks/<task-id>/context.json


the coder
./target/release/hugind agent run agent/coder -- \
  --task <project>/.worktrees/<task-id>/tasks/<task-id>/task.md \
  --output <project>/.worktrees/<task-id>/tasks/<task-id>/output.diff \
  --context <project>/.worktrees/<task-id>/tasks/<task-id>/context.json \ 
  --project <project>

the auditor
./target/release/hugind agent run agent/coder_audit -- \
  --task <project>/.worktrees/<task-id>/tasks/<task-id>/task.md \
  --issue <project>/.worktrees/<task-id>/tasks/<task-id>/issue.md \
  --diff <project>/.worktrees/<task-id>/tasks/<task-id>/output.diff \
  --context <project>/.worktrees/<task-id>/tasks/<task-id>/context.json \ 
  --project <project>

the sould purpose of auditor is to double check if coder did as task says so or not

./target/release/hugind agent run agent/coder_docs -- \
  --task <project>/.worktrees/<task-id>/tasks/<task-id>/task.md \
  --diff <project>/.worktrees/<task-id>/tasks/<task-id>/output.diff \
  --docs <project>/.worktrees/<task-id>/tasks/<task-id>/docs.md


end of flow, the docs.md and output.diff should be in review screen for human to review and approve or reject

if approved

apply patch on code
./target/release/hugind agent run agent/coder_patcher -- \
  --diff <project>/.worktrees/<task-id>/tasks/<task-id>/output.diff \
  --project <project>

merge the code into base code

./target/release/hugind agent run agent/coder_merger -- \
  --repo <project> \
  --worktree <project>/.worktrees/<task-id> \
  --branch <task-id> \
  --docs <project>/.worktrees/<task-id>/tasks/<task-id>/docs.md \
  --diff <project>/.worktrees/<task-id>/tasks/<task-id>/output.diff

cleanup

./target/release/hugind agent run agent/coder_setup -- \
  --repo <project> \
  --run-id <task-id> \
  --mode cleanup \
  --delete-branch true \
  --force true


if rejected just run clean up
