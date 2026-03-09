# chrome_tester

Autonomous web tester agent built for Hugind using the official Chrome DevTools MCP server.

## What It Does
- Discovers Chrome MCP tools dynamically via `tools.list()`.
- Uses an LLM loop to choose the next action.
- Acts using Chrome MCP tools (`take_snapshot`, `click`, `fill`, `wait_for`, etc.).
- Detects blocking takeover overlays/ad-like frames (`detect_blocking_overlay`).
- Attempts recovery (`dismiss_blocking_overlay`) by clicking close controls or hiding blockers in test mode.
- Extracts phone numbers from page state (`extract_phone_numbers`) and prints candidates in console.
- Logs every step under `logs/`.

## Prerequisites
- Google Chrome installed.
- `chrome-devtools-mcp` available through npm/npx.

## Run

```bash
./target/release/hugind agent run agent/chrome_tester -- \
  --goal "Open example.com and verify the main heading is visible" \
  --start-url "https://example.com" \
  --max-steps 20
```

## CLI Options
- `--goal <text>`: testing objective.
- `--start-url <url>`: optional initial navigation target.
- `--workflow <path.json>`: run a multi-step workflow file.
- `--max-steps <n>`: max loop iterations.
- `--step-delay <seconds>`: minimum delay between steps.

## Workflow Mode

Run:

```bash
./target/release/hugind agent run agent/chrome_tester -- \
  --workflow agent/chrome_tester/examples/wikipedia_complex_workflow.json
```

Marrakech restaurant phone workflow:

```bash
./target/release/hugind agent run agent/chrome_tester -- \
  --workflow agent/chrome_tester/examples/marrakech_restaurant_phone_workflow.json
```

Reddit research workflow (Google-block resistant):

```bash
./target/release/hugind agent run agent/chrome_tester -- \
  --workflow agent/chrome_tester/examples/reddit_research_workflow.json
```

Deterministic local workflow (no external bot blocking):

1. Start local static test server:

```bash
python3 -m http.server 8787 --directory agent/chrome_tester/test_site
```

2. In another terminal run:

```bash
./target/release/hugind agent run agent/chrome_tester -- \
  --workflow agent/chrome_tester/examples/local_restaurant_phone_workflow.json
```

Workflow JSON shape:

```json
{
  "name": "workflow_name",
  "continueOnFailure": false,
  "steps": [
    {
      "name": "step name",
      "startUrl": "https://example.com",
      "maxSteps": 15,
      "goal": "What to validate",
      "checks": ["extra check 1", "extra check 2"],
      "instructions": "optional instructions"
    }
  ]
}
```

## Notes
- This agent intentionally uses only MCP browser tools (no shell automation).
- Default mode is visible Chrome (no `--headless`) for easier visual debugging.
- The manifest currently keeps `--isolated`, so it uses a separate profile/session.
- If your environment sandboxes spawned processes heavily, run Chrome with remote debugging and configure MCP with `--browser-url`.
- Tool names can evolve across `chrome-devtools-mcp` versions, so this agent resolves capabilities from runtime discovery instead of hardcoding one exact schema.
- If `tools.list()` is incompatible in your MCP bridge, the agent now falls back to known Chrome tool-name variants automatically.
