# Hugind Tool Calling Flow

## Key Insight

hugind server can either run locally on user machine or on a remote server on network.
The agent YAML can point to a local config name, or directly to a server URL.

The LLM cannot call tools directly — the model may be running on a remote server,
so "listing a directory" from the LLM side is impossible.
Instead, the LLM tells the agent what tools to run, and the agent executes them locally.

---

## 1. Agent Setup Phase

When an agent runs in `mode: agentic` (`runner.rs:471`), two separate init steps happen:

**Step 1 — JS Runtime + standard globals** (`runner.rs:480`, `globals.rs:8-27`):
```
JsRuntime::new_with_team()
  → install_globals()
      → sys::install()     — print, print_raw, eprint, input, hugind_version
      → llm::install()     — ask_llm
      → net::install()     — fetch, http_get, http_post
      → shell::install()   — run_command (ASYNC), runCommand (ASYNC), spawn (ASYNC)
      → fs::install()      — fs.read_text, fs.write_text, fs.list_dir, fs.cwd, fs.is_dir (all SYNC)
      → tools::install()   — register_tools_json, get_tool_results
      → team::install()    — memory.get/set, messages.send/receive (if team context)
```

**Step 2 — Agentic globals** (`runner.rs:491`, `agentic.rs` via `capabilities/agentic.rs`):
```
agentic_cap::install()
  → creates __tool_executors object (map to store JS execute functions)
  → creates register_tool(def) JS shim:
      - calls __register_tool_inner(name, description, params_json) → Rust ToolRegistry
      - stores def.execute in __tool_executors[name]
  → creates set_system_prompt(prompt) → Rust ToolRegistry
  → creates set_max_turns(n) → Rust ToolRegistry
```

**Step 3 — Entry point execution** (`runner.rs:496`):
```
js.run_module(entry_path)   — evaluates main.js as ES module
js.wait_idle()              — drains event loop
```

After this, ToolRegistry should contain all tools and the system prompt.

**Key files:**
- `src/core/orchestrator/runner.rs:460-534` — agentic mode orchestration
- `src/core/js/capabilities/agentic.rs` — register_tool shim, set_system_prompt
- `src/core/orchestrator/agentic.rs` — ToolRegistry, AgentTool, ParsedToolCall structs
- `src/core/js/globals.rs` — install_globals ordering

---

## 2. System Prompt Construction

Built in `run_agentic_loop_with_js()` at `runner.rs:584-601`:

```
system = registry.get_system_prompt()          // from set_system_prompt() in main.js
system += build_skill_catalog(installed_skills) // summaries from ~/.hugind/skills/
system += registry.tools_prompt()              // tool descriptions (see below)
system += activate_skill tool (if skills exist)
```

`tools_prompt()` (`agentic.rs:62-94`) generates:
```
You have tools. To use one: <tool_call>{"name":"tool_name","args":{...}}</tool_call>
When done, respond without tool_call tags.

- run(command): Run a shell command and return its output.
- read_file(path): Read the full contents of a file.
- search(path, pattern): Search for a text pattern in files.
```

Property names are extracted from the JSON schema `parameters.properties` keys.

---

## 3. LLM API Request

Standard OpenAI-compatible chat completion (`runner.rs:619-632`):

```
POST {backend_url}/chat/completions

{
  "model": "model-name",
  "messages": [
    {"role": "system", "content": "<assembled system prompt>"},
    {"role": "user", "content": "<user prompt>"}
  ],
  "stream": false
}
```

- Headers may include `X-Session-ID` for session management (fresh/resume/stateless modes)
- User prompt is built by `build_agentic_prompt()` (`runner.rs:861-893`) from CLI args
  - `--goal 'text'` becomes user prompt `--goal text` (args joined)

**Key file:** `src/core/config/backend.rs` — ResolvedBackend with base_url, model, session

---

## 4. Response Processing

### 4a. Thinking Tag Removal

Models with thinking enabled (e.g. Qwen with `enable_thinking: true`) wrap internal reasoning in `<think>...</think>` tags.

`strip_thinking()` (`agentic.rs`) removes these before tool parsing and final output:
- Strips closed blocks: `<think>reasoning...</think>` → removed
- Strips unclosed blocks: `<think>still thinking...` (at end of response) → removed
- Raw content (with thinking) is preserved in message history so the model sees its own context

Applied at `runner.rs:649`: `let content = strip_thinking(raw_content);`

### 4b. Tool Call Parsing

The LLM responds with content that may contain tool call blocks in various formats:

**Standard format:**
```
<tool_call>{"name":"read_file","args":{"path":"/tmp/foo.py"}}</tool_call>
```

**Gemma-style format** (produced by gemma-4-26b):
```
<|tool_call>call:run{command: "ls /Applications"}<tool_call|>
```

**Other variant:**
```
<|tool_call|>call:read_file{"path": "/tmp/x"}<|/tool_call|>
```

Parsing (`parse_tool_calls()` in `agentic.rs`):
1. Try standard tags `<tool_call>...</tool_call>` first
2. If no matches, try Gemma tags `<|tool_call>...<tool_call|>`
3. If no matches, try `<|tool_call|>...<|/tool_call|>`

For each matched block, two inner formats are tried:
- **JSON format**: `{"name":"tool","args":{...}}` — via `try_parse_json_tool_call()`
- **call:name format**: `call:run{command: "ls"}` — via `try_parse_call_colon_format()`

JSON parsing has a fallback through `fix_unquoted_keys()` which handles:
- Unquoted keys: `{name: "read_file"}` → `{"name": "read_file"}`
- Trailing commas before `}` or `]`

Returns `Vec<ParsedToolCall>` with `name: String` and `args: JsonValue`

---

## 5. Tool Execution (Local, on Agent Side)

For each parsed tool call (`runner.rs:662-698`):

**Built-in: `activate_skill`** (`runner.rs:672-681`):
- Loads full instructions from `~/.hugind/skills/{name}/SKILL.md`
- Result string sent back to LLM as context

**Agent-registered tools** via `execute_js_tool()` (`runner.rs:761-860`):

1. Set JS globals: `__tc_name`, `__tc_args`, `__tc_done = false`, `__tc_result = null`
2. Eval an async wrapper script (`runner.rs:789-813`):
   ```js
   (async function() {
       try {
           var fn = __tool_executors[__tc_name];
           if (!fn) { __tc_result = "Error: ..."; __tc_done = true; return; }
           var result = fn(__tc_args);
           if (result && typeof result.then === 'function') {
               result = await result;  // handles async execute callbacks
           }
           __tc_result = (result as string) || "OK";
       } catch(e) {
           __tc_result = "Error: " + e.message;
       }
       __tc_done = true;
   })();
   ```
3. Rust polls the JS event loop (`runner.rs:820-836`):
   - Up to 600 iterations, 10ms sleep between each
   - Checks `__tc_done` global each iteration
4. Read `__tc_result` string from JS globals

**Important:** The wrapper's `await` on line `result = await result` is what makes async execute callbacks work. If the execute function returns a Promise (from `async function`), it gets awaited here.

---

## 6. Results Sent Back to LLM

Tool results are formatted and appended as a user message (`runner.rs:700-704`):

```json
{
  "role": "user",
  "content": "Tool results:\n\n[read_file] pub fn main() { ... }\n\n[run] Android Studio.app"
}
```

The conversation history grows:
1. system: prompt + tool descriptions
2. user: original request
3. assistant: response with `<tool_call>` blocks
4. user: "Tool results:\n\n[tool_name] result..."
5. (loop continues)

---

## 7. Loop Termination

The loop (`runner.rs:613-705`) ends when:

1. **No tool calls** in LLM response → `strip_tool_calls(content)` returns final text
2. **Max turns reached** → sends "You have reached the maximum number of turns. Please provide your final answer now." and does one final LLM call (`runner.rs:712-743`)
3. **HTTP error** → returns error immediately

Max turns priority: `set_max_turns()` in JS > `max_turns` in agent.yaml > default 10.

---

## 8. Complete Data Flow

```
agent.yaml loaded
       ↓
JsRuntime::new_with_team()
  → install_globals()        ← sys, shell, fs, net, tools, team
  → agentic_cap::install()   ← register_tool, set_system_prompt, set_max_turns
       ↓
js.run_module(main.js)
  → main.js calls set_system_prompt("...")
  → main.js calls register_tool({...}) × N
  → execute fns stored in __tool_executors
       ↓
js.wait_idle()
       ↓
ToolRegistry has N tools + system prompt
       ↓
Build full system prompt = custom + skills + tools_prompt()
Build user prompt from CLI args
       ↓
┌──→ POST /chat/completions { model, messages, stream:false }
│              ↓
│    LLM response content string
│              ↓
│    strip_thinking(content) — remove <think>...</think>
│              ↓
│    parse_tool_calls(content) — find <tool_call> or <|tool_call> variants
│              ↓
│    tool calls found? ──NO──→ strip_tool_calls() → return final text
│         YES
│              ↓
│    for each tool call:
│      execute_js_tool() → async wrapper → __tool_executors[name](args)
│      poll event loop until __tc_done
│      read __tc_result
│              ↓
│    format: "Tool results:\n\n[name] result\n\n[name2] result2"
│    append as {"role":"user"} message
│              ↓
└──────────────┘  (next turn, up to max_turns)
```

---

## 9. Important Design Decisions

- **Text-based tool format** (`<tool_call>` tags in plain text) instead of OpenAI function_calling API — works with any LLM backend, including small local models that don't support function calling
- **Multi-format tool call parsing** — supports standard `<tool_call>` tags, Gemma-style `<|tool_call>call:name{...}<tool_call|>`, and other variants. Lenient JSON via `fix_unquoted_keys()` handles unquoted keys and trailing commas.
- **Thinking tag stripping** — `<think>...</think>` blocks are removed before tool parsing and output, but preserved in message history for model context continuity
- **Tools run on the agent side, not the LLM side** — the LLM only describes what to do; execution happens locally where the filesystem/shell is accessible
- **Two-stage skill system** — catalog summary in system prompt, full instructions loaded on-demand via `activate_skill` tool call
- **JS execution with Rust polling** — rquickjs is not Send/Sync, so Rust uses global variables + event loop polling to exchange data with JS
- **Async wrapper for tool execution** — the eval'd `(async function(){...})()` handles both sync and async execute callbacks transparently

---

## 10. Critical Gotchas for Agent JS Authors

### `run_command()` is async — cannot be called at module top level
`run_command()` is registered with rquickjs `Async()` wrapper (`shell.rs:204-211`). This means:
- **At module top level**: `run_command("uname")` returns a **Promise**, NOT a string. Top-level await is not supported. Calling `.trim()` or `.split()` on the result fails.
- **Inside tool execute callbacks**: you MUST use `async function` + `await`:
  ```js
  // CORRECT
  execute: async function(args_json) {
    var args = JSON.parse(args_json);
    var result = await run_command(args.command);
    return result;
  }

  // WRONG — run_command returns Promise, .split() fails
  execute: function(args_json) {
    var args = JSON.parse(args_json);
    var result = run_command(args.command);
    return result.split("\n").length;
  }
  ```
- The tool executor wrapper handles Promise results via `await`, so `async function` works.

### Module evaluation errors are silent
If the JS entry point throws during `module.eval()`, the error may be swallowed. The module returns Ok but 0 tools are registered and system prompt is empty. Always wrap risky top-level code in try/catch. Use `HUGIND_TRACE=1` to verify tool count and prompt content.

### `fs` functions are synchronous, `shell` functions are async
- **Sync (safe at top level):** `fs.read_text()`, `fs.write_text()`, `fs.list_dir()`, `fs.cwd()`, `fs.is_dir()`, `fs.mkdir()`
- **Async (must await in callbacks, cannot use at top level):** `run_command()`, `runCommand()`, `spawn()`

### Debugging with HUGIND_TRACE
Set `HUGIND_TRACE=1` environment variable to see:
- Tool count after entry point execution
- Full system prompt and user prompt content
- Each turn: message count, response status/timing, content preview, tool call count
- Each tool execution: name, args, duration, result length
