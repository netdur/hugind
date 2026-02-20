# Task: Add error dialog on login failure

## Objective
When login fails (the `catch` path in `submit()`), show a user-facing error dialog.

## Requirements
- Use the existing dialog helper in `src/ui/dialog.js`.
- Show a dialog with:
  - title: `Login failed`
  - message: use the best available error message:
    - prefer `e.message` if present
    - otherwise use `Login failed. Please try again.`
- Keep the existing inline hint behavior (`serverHint`) unchanged.
- Do not change API behavior or request format.
- Only modify files inside `src/login/` and/or `src/ui/` if necessary.
- Keep the change minimal and localized.

## Output
Produce a unified diff patch that implements the change.
