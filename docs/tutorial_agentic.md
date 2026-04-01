# Building Agentic Agents

This tutorial walks through building agents that use Hugind's agentic mode -- where the agent registers tools and a system prompt, and the runtime drives the LLM tool-use loop automatically.

## Prerequisites

- Hugind installed (`hugind --version`)
- A model downloaded and a config created (see [CLI: config](cli_config.md))
- A running server (`hugind server start <config>`)

## Part 1: Your First Agentic Agent

### 1.1 Create the agent directory

```bash
mkdir -p my-agent
cd my-agent
```

### 1.2 Write agent.yaml

Every agent needs a manifest. For agentic mode, set `mode: agentic`:

```yaml
name: my-agent
version: "1.0"
entry_point: main.js
mode: agentic
max_turns: 10

permissions:
  filesystem:
    allow: true
    read: true
    write: true
    create: true
    allow_outside_agent_root: true
  shell:
    allow: false
```

Key fields:

| Field | Purpose |
|---|---|
| `mode: agentic` | Runtime drives the LLM loop; you just register tools |
| `max_turns` | Safety limit on LLM round-trips (default 10) |
| `permissions` | What the agent is allowed to touch |

### 1.3 Write main.js

In agentic mode, your entry point runs once to set up the agent. The runtime takes over after that.

```js
// Set the system prompt -- this tells the LLM who it is
set_system_prompt(`You are a file assistant. You can read and write files.
Always confirm what you did after each action.`);

// Register tools the LLM can call
register_tool({
  name: "read_file",
  description: "Read the contents of a file",
  parameters: {
    type: "object",
    properties: {
      path: { type: "string", description: "File path to read" }
    }
  },
  execute: (args) => {
    const parsed = JSON.parse(args);
    return fs.read_text(parsed.path);
  }
});

register_tool({
  name: "write_file",
  description: "Write content to a file",
  parameters: {
    type: "object",
    properties: {
      path: { type: "string", description: "File path" },
      content: { type: "string", description: "Content to write" }
    }
  },
  execute: (args) => {
    const parsed = JSON.parse(args);
    fs.write_text(parsed.path, parsed.content);
    return "Written to " + parsed.path;
  }
});
```

### 1.4 Run it

```bash
hugind agent run my-agent --prompt "Create a file called hello.txt with the text 'Hello, World!'"
```

The runtime will:
1. Run `main.js` to collect your tools and system prompt
2. Send the user's prompt to the LLM along with the tool definitions
3. When the LLM responds with a `<tool_call>`, execute the matching tool
4. Feed the tool result back to the LLM
5. Repeat until the LLM responds without a tool call, or `max_turns` is reached

Enable tracing to see every step:

```bash
HUGIND_TRACE=1 hugind agent run my-agent --prompt "Create hello.txt"
```

---

## Part 2: Tool Design Patterns

### 2.1 Tool anatomy

Every tool needs four fields:

```js
register_tool({
  name: "tool_name",           // unique identifier the LLM uses
  description: "What it does", // the LLM reads this to decide when to use the tool
  parameters: {                // JSON Schema describing the arguments
    type: "object",
    properties: {
      arg1: { type: "string", description: "..." },
      arg2: { type: "number", description: "..." }
    }
  },
  execute: (args) => {         // called when the LLM invokes the tool
    const parsed = JSON.parse(args);
    // do work, return a string
    return "result";
  }
});
```

The `execute` function:
- Receives a JSON string of arguments
- Must return a string (the tool result shown to the LLM)
- Can call any Hugind API (`fs.*`, `llm.*`, `run_command`, etc.)
- Errors thrown here are caught and reported to the LLM

### 2.2 Shell tool with permission gating

```js
register_tool({
  name: "run",
  description: "Execute a shell command and return its output",
  parameters: {
    type: "object",
    properties: {
      command: { type: "string", description: "Shell command to execute" }
    }
  },
  execute: (args) => {
    const parsed = JSON.parse(args);
    return run_command(parsed.command);
  }
});
```

For this to work, `agent.yaml` must allow shell access:

```yaml
permissions:
  shell:
    allow: true
    timeout: "30s"
    max_output: "1MB"
```

You can restrict which commands are available:

```yaml
permissions:
  shell:
    allow: true
    whitelist: ["python3", "node", "grep", "find"]
```

Or block dangerous ones:

```yaml
permissions:
  shell:
    allow: true
    blacklist: ["rm", "curl", "wget"]
```

### 2.3 Search tool wrapping grep

```js
register_tool({
  name: "search",
  description: "Search for a pattern in files under a directory",
  parameters: {
    type: "object",
    properties: {
      pattern: { type: "string", description: "Text pattern to search for" },
      path: { type: "string", description: "Directory to search in" }
    }
  },
  execute: (args) => {
    const parsed = JSON.parse(args);
    const dir = parsed.path || ".";
    return run_command(`grep -rn "${parsed.pattern}" "${dir}" || true`);
  }
});
```

### 2.4 Directory-aware write tool

Auto-create parent directories before writing:

```js
register_tool({
  name: "write_file",
  description: "Write content to a file, creating parent directories if needed",
  parameters: {
    type: "object",
    properties: {
      path: { type: "string", description: "File path" },
      content: { type: "string", description: "File content" }
    }
  },
  execute: (args) => {
    const parsed = JSON.parse(args);
    const parts = parsed.path.split("/");
    if (parts.length > 1) {
      const dir = parts.slice(0, -1).join("/");
      fs.mkdir(dir, true);
    }
    fs.write_text(parsed.path, parsed.content);
    return "Written to " + parsed.path;
  }
});
```

---

## Part 3: Controlling the LLM

### 3.1 System prompt

The system prompt shapes how the LLM behaves. Be specific:

```js
set_system_prompt(`You are a Python developer. You write clean, tested code.

Rules:
- Always include type hints
- Write one file at a time
- After writing code, verify it runs with: python3 -c "import <module>"
- If a test fails, fix the code and re-test before reporting done`);
```

### 3.2 Max turns

Override the `max_turns` from agent.yaml at runtime:

```js
// Complex tasks need more turns
set_max_turns(20);
```

### 3.3 Backend configuration

Point the agent at a specific model in `agent.yaml`:

```yaml
backend:
  config: qwen-32b    # references ~/.hugind/configs/qwen-32b.yml
```

Or use a direct URL:

```yaml
backend:
  url: "http://127.0.0.1:9090/v1"
```

Session management:

```yaml
backend:
  config: gemma-4b
  session:
    mode: stateless   # no session persistence (default)
    # mode: fresh     # new session each run
    # mode: resume    # resume a named session
    # id: my-session
```

---

## Part 4: Multi-Agent Teams

Teams let multiple agents collaborate on a goal. Each agent has its own tools and permissions, but they share memory and can exchange messages.

### 4.1 Architecture

```
hugind agent team "Build an API" --agents agent/ma-architect,agent/ma-developer,agent/ma-tester

  1. Coordinator LLM decomposes the goal into tasks
  2. Tasks are assigned to agents with a dependency DAG
  3. Independent tasks run in parallel
  4. Agents share memory and messages
```

### 4.2 Shared memory

Agents store and retrieve data through a shared key-value store. Keys are namespaced by agent name.

**Writing** (from the architect agent):

```js
// Stores as "ma-architect/spec"
memory.set("spec", specContent);
```

**Reading** (from the developer agent):

```js
// Read another agent's data using the full key
const spec = memory.get("ma-architect/spec");
```

**Listing all entries:**

```js
const all = memory.list();    // JSON object of all key-value pairs
const md = memory.summary();  // markdown summary grouped by agent
```

### 4.3 Inter-agent messaging

Point-to-point:

```js
messaging.send("ma-developer", "The spec is ready, please start implementation");
```

Broadcast to all:

```js
messaging.broadcast("I've completed the review, see shared memory for results");
```

Receive unread messages:

```js
const msgs = JSON.parse(messaging.receive());
// [{from: "ma-architect", to: "ma-developer", content: "..."}]
```

### 4.4 Dynamic task spawning

Agents can create new tasks at runtime:

```js
const result = JSON.parse(tasks.spawn(JSON.stringify({
  title: "Fix failing test",
  description: "The login endpoint test returns 401, investigate and fix",
  assignee: "ma-developer",
  depends_on: ["task-id-of-current-task"]
})));

if (result.ok) {
  print("Spawned task: " + result.id);
} else {
  print("Failed: " + result.error);
}
```

The coordinator validates the dependency graph on every spawn. Circular dependencies (A depends on B, B depends on A) are detected immediately via DFS cycle detection, and `tasks.spawn` returns `{ok: false, error: "Dependency cycle detected ..."}`. The invalid task is not added to the queue.

### 4.5 Building a team from scratch

**Step 1: Create the architect**

`agent/my-architect/agent.yaml`:
```yaml
name: my-architect
version: "1.0"
entry_point: main.js
mode: agentic
max_turns: 6
permissions:
  filesystem:
    allow: true
    read: true
    write: true
    create: true
    allow_outside_agent_root: true
```

`agent/my-architect/main.js`:
```js
set_system_prompt(`You are a software architect.
Given a goal, produce a clear technical spec.
Save the spec to shared memory so other agents can read it.`);

register_tool({
  name: "write_spec",
  description: "Save a technical specification to shared memory and to a file",
  parameters: {
    type: "object",
    properties: {
      content: { type: "string", description: "The full spec in markdown" }
    }
  },
  execute: (args) => {
    const parsed = JSON.parse(args);
    memory.set("spec", parsed.content);
    fs.write_text("spec.md", parsed.content);
    return "Spec saved to shared memory and spec.md";
  }
});

register_tool({
  name: "read_file",
  description: "Read a file",
  parameters: {
    type: "object",
    properties: {
      path: { type: "string", description: "File path" }
    }
  },
  execute: (args) => {
    const parsed = JSON.parse(args);
    return fs.read_text(parsed.path);
  }
});
```

**Step 2: Create the developer**

`agent/my-developer/agent.yaml`:
```yaml
name: my-developer
version: "1.0"
entry_point: main.js
mode: agentic
max_turns: 15
permissions:
  filesystem:
    allow: true
    read: true
    write: true
    create: true
    allow_outside_agent_root: true
  shell:
    allow: true
    timeout: "60s"
    max_output: "1MB"
```

`agent/my-developer/main.js`:
```js
const spec = memory.get("my-architect/spec");

let context = "You are a developer. Implement code according to the spec.";
if (spec && spec !== "null") {
  context += "\n\nSpec:\n" + spec;
}

set_system_prompt(context);

register_tool({
  name: "write_file",
  description: "Write content to a file",
  parameters: {
    type: "object",
    properties: {
      path: { type: "string", description: "File path" },
      content: { type: "string", description: "File content" }
    }
  },
  execute: (args) => {
    const parsed = JSON.parse(args);
    const parts = parsed.path.split("/");
    if (parts.length > 1) {
      fs.mkdir(parts.slice(0, -1).join("/"), true);
    }
    fs.write_text(parsed.path, parsed.content);
    return "Written to " + parsed.path;
  }
});

register_tool({
  name: "read_file",
  description: "Read a file",
  parameters: {
    type: "object",
    properties: {
      path: { type: "string", description: "File path" }
    }
  },
  execute: (args) => {
    const parsed = JSON.parse(args);
    return fs.read_text(parsed.path);
  }
});

register_tool({
  name: "run",
  description: "Run a shell command",
  parameters: {
    type: "object",
    properties: {
      command: { type: "string", description: "Command to execute" }
    }
  },
  execute: (args) => {
    const parsed = JSON.parse(args);
    return run_command(parsed.command);
  }
});
```

**Step 3: Create the tester**

`agent/my-tester/agent.yaml`:
```yaml
name: my-tester
version: "1.0"
entry_point: main.js
mode: agentic
max_turns: 8
permissions:
  filesystem:
    allow: true
    read: true
  shell:
    allow: true
    timeout: "30s"
    max_output: "1MB"
```

`agent/my-tester/main.js`:
```js
set_system_prompt(`You are a QA engineer. Verify the implementation works correctly.
Run the code, check the output, and report pass/fail for each test case.`);

register_tool({
  name: "read_file",
  description: "Read a file",
  parameters: {
    type: "object",
    properties: {
      path: { type: "string", description: "File path" }
    }
  },
  execute: (args) => {
    const parsed = JSON.parse(args);
    return fs.read_text(parsed.path);
  }
});

register_tool({
  name: "run",
  description: "Run a shell command",
  parameters: {
    type: "object",
    properties: {
      command: { type: "string", description: "Command to execute" }
    }
  },
  execute: (args) => {
    const parsed = JSON.parse(args);
    return run_command(parsed.command);
  }
});

register_tool({
  name: "list_dir",
  description: "List files in a directory",
  parameters: {
    type: "object",
    properties: {
      path: { type: "string", description: "Directory path" }
    }
  },
  execute: (args) => {
    const parsed = JSON.parse(args);
    return fs.list_dir(parsed.path || ".");
  }
});
```

**Step 4: Run the team**

```bash
hugind agent team "Build a Python CLI that converts CSV to JSON" \
  --agents agent/my-architect,agent/my-developer,agent/my-tester
```

Or read the goal from a file:

```bash
hugind agent team --goal-file goal.md \
  --agents agent/my-architect,agent/my-developer,agent/my-tester
```

---

## Part 5: Workflows

Workflows define tasks, dependencies, and agent assignments in a YAML file. They give you explicit control over the execution plan instead of relying on the coordinator LLM.

### 5.1 Basic workflow

`workflow.yaml`:
```yaml
version: 2
name: build-and-test

tasks:
  - title: Write the code
    agent: my-developer
    description: "Implement a function that calculates Fibonacci numbers"

  - title: Test the code
    agent: my-tester
    description: "Test the Fibonacci function with edge cases: 0, 1, 10, 50"
    depends_on: [Write the code]
```

Run it:

```bash
hugind agent run workflow.yaml
```

### 5.2 Parallel tasks

Tasks without dependencies on each other run concurrently:

```yaml
version: 2
name: review-pipeline

tasks:
  - title: Implement feature
    agent: my-developer
    description: "Build the user registration endpoint"

  - title: Test feature
    agent: my-tester
    description: "Test all registration scenarios"
    depends_on: [Implement feature]

  - title: Review code
    agent: my-reviewer
    description: "Review the implementation for security issues"
    depends_on: [Implement feature]
```

Here, "Test feature" and "Review code" both depend on "Implement feature" but not on each other, so they run in parallel.

### 5.3 Multi-model workflows

Assign different backends per task -- use a fast model for simple work, a stronger model for complex reasoning:

```yaml
version: 2
name: multi-model

backends:
  fast: gemma-4b
  smart: qwen-32b

tasks:
  - title: Scaffold project
    agent: my-developer
    backend: fast
    description: "Create the project structure and boilerplate"

  - title: Implement logic
    agent: my-developer
    backend: smart
    description: "Implement the core business logic"
    depends_on: [Scaffold project]

  - title: Write tests
    agent: my-tester
    backend: fast
    description: "Write and run tests"
    depends_on: [Implement logic]
```

---

## Part 6: Permissions Deep Dive

Permissions are declared in `agent.yaml` and enforced at the runtime level. An agent cannot bypass them.

### 6.1 Minimal permissions (read-only auditor)

```yaml
permissions:
  network:
    allow: false
  filesystem:
    allow: true
    read: true
    write: false
    delete: false
  shell:
    allow: false
```

### 6.2 Network-scoped agent

```yaml
permissions:
  network:
    allow: true
    allowed_domains: ["api.github.com", "httpbin.org"]
    block_private_networks: true
    timeout: "10s"
    max_response_bytes: "5MB"
  filesystem:
    allow: false
  shell:
    allow: false
```

### 6.3 Sandboxed shell

On macOS, shell commands run inside `sandbox-exec` automatically. You can further restrict with:

```yaml
permissions:
  shell:
    allow: true
    whitelist: ["python3", "node", "npm"]
    timeout: "30s"
    max_output: "1MB"
    env_clear: true
    working_dir: "/tmp/sandbox"
```

### 6.4 Filesystem path scoping

```yaml
permissions:
  filesystem:
    allow: true
    read: true
    write: true
    allow_outside_agent_root: true
    allowed_paths:
      - "/Users/me/projects/target-repo"
      - "/tmp"
    denied_paths:
      - "/Users/me/.ssh"
      - "/Users/me/.env"
```

---

## Part 7: Script Mode vs Agentic Mode

Not every agent needs agentic mode. For simple tasks, script mode gives you full control.

### Script mode

```yaml
mode: script   # or omit mode entirely (script is default)
```

```js
export default async function(input) {
  const prompt = input.args.prompt || "Hello";
  const response = llm.chat(prompt);
  return JSON.parse(response);
}
```

You call `llm.chat()` directly. You control the loop. Good for:
- Single-shot LLM calls (OCR, classification, extraction)
- Custom multi-step pipelines
- When you need precise control over what gets sent to the LLM

### Agentic mode

```yaml
mode: agentic
```

```js
set_system_prompt("You are a helpful assistant.");
register_tool({ name: "...", ... });
```

The runtime calls the LLM in a loop. Good for:
- Open-ended tasks where the LLM decides the steps
- Agents that need to react to intermediate results
- Multi-tool workflows where the LLM orchestrates

### When to use which

| Use case | Mode |
|---|---|
| OCR / image analysis | script |
| Data extraction | script |
| Code generation with test-fix loops | agentic |
| File manipulation tasks | agentic |
| Multi-step research | agentic |
| Simple Q&A wrapper | script |

---

## Part 8: WASM Agents

Everything above also works for WebAssembly agents. WASM agents have access to the same team, messaging, task, and agentic APIs.

### 8.1 WASM agent.yaml

```yaml
name: my-wasm-agent
version: "1.0"
entry_point: main.wasm
mode: agentic
max_turns: 10

wasm:
  runtime_fs_mode: host_filesystem
  resources:
    memory: "256MB"
    timeout: "60s"
    cpu: "1000000000"

permissions:
  filesystem:
    allow: true
    read: true
    write: true
    create: true
    allow_outside_agent_root: true
```

### 8.2 WASM SDK usage (AssemblyScript)

Import from the SDK (see [wasm_sdk.ts](wasm_sdk.ts)):

```typescript
import {
  setSystemPrompt,
  registerTool,
  memoryGet,
  memorySet,
  messagingSend,
  messagingReceive,
  tasksSpawn,
  setMaxTurns,
  fsReadText,
  fsWriteText,
  runCommand,
} from "./hugind";

// Same pattern as JS -- register tools and system prompt
setSystemPrompt("You are a helpful assistant.");

registerTool(JSON.stringify({
  name: "read_file",
  description: "Read a file",
  parameters: {
    type: "object",
    properties: {
      path: { type: "string", description: "File path" }
    }
  }
}));

// Use shared memory
const spec = memoryGet("architect/spec");
memorySet("status", "ready");

// Send messages
messagingSend("developer", "The spec is ready");
```

Note: in WASM, tool `execute` functions are not registered inline. Instead, the runtime calls back into exported WASM functions when a tool is invoked. See [WASM Runtime](wasm_runtime.md) for the callback protocol.

### 8.3 WASM resource limits

```yaml
wasm:
  resources:
    memory: "256MB"   # max memory the module can allocate
    timeout: "60s"    # wall-clock timeout for entire execution
    cpu: "1B"         # fuel budget (~instruction count)
```

---

## Part 9: MCP Tool Integration

Agents can use external tools via the Model Context Protocol (MCP).

### 9.1 Declare MCP servers in agent.yaml

```yaml
dependencies:
  mcp:
    - name: filesystem
      required: true
      transport: stdio
      command: npx
      args: ["-y", "@anthropic/mcp-filesystem"]
      env:
        ROOT_DIR: "/Users/me/data"
```

### 9.2 Use MCP tools in your agent

```js
// List available tools from all MCP servers
const tools = JSON.parse(tools.list());
print("Available: " + tools.map(t => t.name).join(", "));

// Call a tool
const result = tools.call("filesystem:read_file", { path: "/Users/me/data/config.json" });
```

MCP tools are available in both script and agentic modes, and in both JS and WASM runtimes.

---

## Part 10: Debugging

### HUGIND_TRACE

Set `HUGIND_TRACE=1` to see every runtime event:

```bash
HUGIND_TRACE=1 hugind agent run my-agent --prompt "do something"
```

Output includes:
- Tool registrations
- Each LLM request/response
- Tool calls and results
- Memory and messaging operations

### Session logs

Every agent run is logged to `~/.hugind/logs/agents/`:

```
[2026-02-10T12:29:33.526Z] agent.run.start name=my-agent entry=main.js
[2026-02-10T12:29:33.530Z] host.fs.read_text path=spec.md
[2026-02-10T12:29:35.014Z] host.llm.chat_stream input=object messages=Some(1)
[2026-02-10T12:29:41.198Z] agent.run.complete status=ok
```

Specify a custom log path:

```bash
hugind agent run my-agent --log-file ./debug.log --prompt "test"
```

### Common issues

| Problem | Fix |
|---|---|
| "permission denied" on fs/shell/net | Check `agent.yaml` permissions section |
| LLM not calling tools | Improve tool descriptions; check system prompt clarity |
| Agent loops without progress | Lower `max_turns`; make system prompt more directive |
| "No backend configured" | Add `backend:` section to agent.yaml or start a server |
| Tool returns error to LLM | Check the `execute` function; errors are shown to the LLM as tool results |

---

## Quick Reference

### Agentic globals (JS)

```js
set_system_prompt(prompt)          // set the system prompt
register_tool({ name, description, parameters, execute })
set_max_turns(n)                   // override max turns
```

### Team globals (JS)

```js
memory.set(key, value)             // write to shared memory
memory.get("agent/key")            // read from shared memory
memory.list()                      // all entries as JSON
memory.summary()                   // markdown summary

messaging.send(to, content)        // point-to-point
messaging.broadcast(content)       // to all agents
messaging.receive()                // unread messages

tasks.spawn(json)                  // create dynamic task
```

### Agentic hostcalls (WASM)

```
hugind.register_tool(json_ptr, json_len)
hugind.set_system_prompt(ptr, len)
hugind.set_max_turns(n)
```

### Team hostcalls (WASM)

```
hugind.memory_set(key_ptr, key_len, val_ptr, val_len)
hugind.memory_get(key_ptr, key_len) -> i64
hugind.memory_list() -> i64
hugind.memory_summary() -> i64
hugind.messaging_send(to_ptr, to_len, content_ptr, content_len)
hugind.messaging_broadcast(ptr, len)
hugind.messaging_receive() -> i64
hugind.tasks_spawn(json_ptr, json_len) -> i64
```
