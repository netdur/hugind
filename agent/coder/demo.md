# step
(base) adel@192 hugind % pwd
/Users/adel/Workspace/hugind
(base) adel@192 hugind % ls /Users/adel/Workspace/atlas_workspace/test
src
(base) adel@192 hugind % ./target/release/hugind agent run agent/coder_setup -- \
  --repo /Users/adel/Workspace/atlas_workspace/test \
  --run-id trial-002 \
  --mode build
Checking server health at http://127.0.0.1:8080/v1/monitor...
Server is up. Starting agent...
[coder_setup] success: Worktree created

# add task
(base) adel@192 hugind % cp agent/coder/tests/task.md /Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002/task.md
(base) adel@192 hugind % ls /Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002
src	task.md

# implement task
(base) adel@192 hugind % ./target/release/hugind agent run agent/coder --cwd /Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002 -- \
  --task task.md \
  --output output.diff \
  --project src
Checking server health at http://127.0.0.1:8080/v1/monitor...
Server is up. Starting agent...
[coder] session.mode=fresh
[coder] session.id=b0527e22-17d1-47cb-b46b-1b3ec6208427
[coder] input validated
[coder] host_cwd=/Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002
[coder] cwd=/Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002
[coder] task=/Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002/task.md
[coder] output=/Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002/output.diff
[coder] project=/Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002/src
[coder] output diff cleared
[coder] project tree profile captured
[coder] context collected
[coder] llm iteration 1/9
[coder] llm iteration 2/9
[coder] patch proposed and validated
[coder] diff generated
[coder] success: Generated diff for 1 file(s)

# audit
(base) adel@192 hugind % ./target/release/hugind agent run agent/coder_audit --cwd /Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002 -- \
  --task task.md \
  --issue issue.md \
  --diff output.diff \
  --project src
Checking server health at http://127.0.0.1:8080/v1/monitor...
Server is up. Starting agent...
[coder_audit] success: Audit passed

# document it
(base) adel@192 hugind % ./target/release/hugind agent run agent/coder_docs --cwd /Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002 -- \
  --task task.md \
  --diff output.diff \
  --docs docs.md
Checking server health at http://127.0.0.1:8080/v1/monitor...
Server is up. Starting agent...
[coder_docs] success: docs.md generated

# apply patch
(base) adel@192 hugind % ./target/release/hugind agent run agent/coder_patcher --cwd /Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002 -- \
  --diff output.diff \
  --project . \
  --dry-run
Checking server health at http://127.0.0.1:8080/v1/monitor...
Server is up. Starting agent...
[coder_patcher] success: Dry run parsed 1 file patch(es)
(base) adel@192 hugind % ./target/release/hugind agent run agent/coder_patcher --cwd /Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002 -- \
  --diff output.diff \
  --project .  
Checking server health at http://127.0.0.1:8080/v1/monitor...
Server is up. Starting agent...
[coder_patcher] success: Applied 1 file patch(es)

# verify
(base) adel@192 hugind % more /Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002/output.diff 
diff --git a/src/login/LoginForm.js b/src/login/LoginForm.js
--- a/src/login/LoginForm.js
+++ b/src/login/LoginForm.js
@@ -1,5 +1,6 @@
 import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
 import { loginRequest } from "../auth/api";
+import { showErrorDialog } from "../ui/dialog";
 
 const Status = Object.freeze({
   idle: "idle",
@@ -72,6 +73,10 @@
       // TASK will ask to add a proper error dialog here.
       setStatus(Status.error);
       setServerHint(e && e.message ? e.message : "Login failed");
+      showErrorDialog({
+        title: "Login failed",
+        message: e && e.message ? e.message : "Login failed. Please try again."
+      });
     }
   }, [username, password, onLogin]);
 
(base) adel@192 hugind % more /Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002/docs.md    
## Summary
Add an error dialog on login failure.

## What Changed
- Show a user-facing error dialog when login fails using the existing dialog helper in `src/ui/dialog.js`.
- Use the best available error message: `e.message` if present, otherwise `Login failed. Please try again.`
- Keep the existing inline hint behavior (`serverHint`) unchanged.

## Files Affected
- `src/login/LoginForm.js`

## Tests to run
Not applicable.

# merge
(base) adel@192 hugind % ./target/release/hugind agent run agent/coder_merger -- \
  --repo /Users/adel/Workspace/atlas_workspace/test \
  --worktree /Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002 \
  --branch agent/trial-002 \
  --docs /Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002/docs.md \
  --diff /Users/adel/Workspace/atlas_workspace/test/.worktrees/trial-002/output.diff
Checking server health at http://127.0.0.1:8080/v1/monitor...
Server is up. Starting agent...
[coder_merger] success: Committed and merged

# clean up
(base) adel@192 hugind % ./target/release/hugind agent run agent/coder_setup -- \
  --repo /Users/adel/Workspace/atlas_workspace/test \
  --run-id trial-002 \
  --mode cleanup \
  --delete-branch true \
  --force true
Checking server health at http://127.0.0.1:8080/v1/monitor...
Server is up. Starting agent...
[coder_setup] success: Cleanup completed

# verify
(base) adel@192 test % git status
On branch master
nothing to commit, working tree clean
(base) adel@192 test % more src/login/LoginForm.js | grep "showErrorDialog"
import { showErrorDialog } from "../ui/dialog";
      showErrorDialog({
