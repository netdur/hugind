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
