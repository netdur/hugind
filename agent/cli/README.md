# Agent CLI

## Goal
Build a terminal interface where a user types plain English requests (e.g. “list files by size”), the LLM proposes a command, decides whether user confirmation is required, optionally asks for permission, executes the command, and returns the output to the LLM to respond or request a follow‑up command.

## Plan
1. **Input**
   - Read a natural‑language prompt from the user.
   - Normalize/trim input and handle empty requests.

2. **LLM Command Proposal**
   - Send the user’s request to the LLM with a system prompt that constrains output to a single safe shell command.
   - Validate that the response is a command (not prose), and reject or re‑prompt if invalid.

3. **Confirmation Policy (LLM‑Driven)**
   - The LLM decides whether a command needs user confirmation.
   - Low‑risk, read‑only commands may execute automatically.
   - Destructive, privileged, or ambiguous commands must require explicit approval (e.g. `y/n`).
   - When confirmation is required:
     - Display the proposed command to the user.
     - If denied, send the denial back to the LLM for a revised command or clarification.

4. **Execution**
   - Run the approved command in a controlled environment (current workspace by default).
   - Capture stdout, stderr, and exit code.

5. **LLM Result Handling**
   - Send the command output (and errors/exit code) back to the LLM.
   - Let the LLM produce a final answer or request another command.

6. **Loop and Exit**
   - Continue the request → propose → (confirm if needed) → execute → summarize loop until the user exits.

## JSON Response Schema
The LLM must reply with a single JSON object. No code fences.

Required fields:
- `kind`: `"command"` or `"answer"`
- `command`: a single shell command, or empty when `kind="answer"`
- `confirm`: boolean
- `answer`: final response text, or empty when `kind="command"`

Example (command):
```json
{"kind":"command","command":"ls -lh","confirm":false,"answer":""}
```

Example (answer):
```json
{"kind":"answer","command":"","confirm":false,"answer":"You have 3 NDKs installed."}
```

## Confirmation Policy Rules
When deciding `confirm`:
- `false` for read‑only commands (e.g., `ls`, `pwd`, `whoami`, `cat <file>` in workspace).
- `true` for anything destructive or risky (e.g., `rm`, `mv`, `chmod`, `sudo`, network writes).
- `true` if the command is ambiguous or could access sensitive locations.
- `true` if the command requires elevated privileges or writes outside the workspace.

If `confirm=true`, the CLI must prompt the user before executing.

## How It Works (Flow)
1. User inputs a request.
2. LLM returns JSON (`kind=command` or `kind=answer`).
3. If `command`:
   - If `confirm=true`, ask user `y/n`.
   - Execute the command if approved.
   - Send output back to the LLM for the next step.
4. If `answer`, print and finish.
5. Repeat until user exits.

## Example: “What NDKs do I have?”
1. **User request**: “What NDKs do I have?”
2. **LLM plan**:
   - Identify the OS.
   - Locate Android Studio or SDK directory.
   - Inspect the NDK folder(s).
3. **Commands**:
   - If OS detection is read‑only, run automatically.
   - If scanning specific directories is read‑only, run automatically.
   - If anything could be sensitive or invasive, require confirmation.
4. **React loop**:
   - Execute a safe probe.
   - Send results to the LLM.
   - LLM decides the next command or final answer.

## Non‑Goals (for v1)
- Complex multi‑step scripts generated in one shot.
- Privileged or destructive commands without explicit approval.
- Long‑running background processes.

## Notes
- Keep prompts and command outputs short and well‑structured.
- Always surface the exact command before execution.
