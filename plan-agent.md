# Agent Implementation Plan

Goal: build a set of reusable agents that compose into multi-agent workflows. Each agent is a directory under `agent/` with `agent.yaml` + `main.js`. All agents run in agentic mode against local Hugind servers.

---

## Principles

- Each agent does one thing well
- Agents communicate via shared memory and messaging, not direct coupling
- Tool declarations match the agent's permission scope — no over-permissioning
- System prompts are concise and task-oriented — no filler
- Agents read team context from `get_args()` which includes shared memory and messages from other agents
- All agents are JS-only (no WASM) for simplicity

---

## Agent 1: `ma-reader`

Reads files, lists directories, searches content. The eyes of any workflow.

### Capabilities
- `fs.read(path)` — read file contents
- `fs.list_dir(path)` — list directory entries
- `fs.read_bytes(path)` — read binary (images)
- `run_command("grep ...")` — search within files

### agent.yaml
```yaml
name: ma-reader
version: "1.0"
entry_point: main.js
mode: agentic
max_turns: 8
permissions:
  filesystem:
    allow: true
    read: true
    allow_outside_agent_root: true
  shell:
    allow: true
    whitelist: ["grep", "find", "wc", "head", "tail", "cat"]
```

### Tools registered
- `read_file(path)` — returns file contents as string
- `list_dir(path)` — returns directory listing
- `search(pattern, path)` — grep for pattern in path

### Use cases
- Read a spec before implementing
- Scan a codebase for patterns
- Count lines, find files by name
- Feed into ma-reviewer or ma-tester workflows

---

## Agent 2: `ma-writer`

Writes files and creates directory structures.

### Capabilities
- `fs.write(path, content)` — write file
- `fs.mkdir(path)` — create directory
- `run_command("mkdir -p ...")` — create nested dirs

### agent.yaml
```yaml
name: ma-writer
version: "1.0"
entry_point: main.js
mode: agentic
max_turns: 12
permissions:
  filesystem:
    allow: true
    read: true
    write: true
    create: true
    allow_outside_agent_root: true
  shell:
    allow: true
    whitelist: ["mkdir"]
```

### Tools registered
- `write_file(path, content)` — write/overwrite a file
- `create_dir(path)` — create directory (recursive)
- `read_file(path)` — also needs read to check existing files before writing

### Use cases
- Write code files from a spec
- Create project scaffolding
- Save specs, configs, documentation

---

## Agent 3: `ma-shell`

Runs shell commands — builds, tests, installs dependencies, starts/stops processes.

### Capabilities
- `run_command(cmd)` — execute arbitrary shell command
- `spawn(program, args)` — run a specific program

### agent.yaml
```yaml
name: ma-shell
version: "1.0"
entry_point: main.js
mode: agentic
max_turns: 10
permissions:
  filesystem:
    allow: true
    read: true
  shell:
    allow: true
    timeout: "60s"
    max_output: "1MB"
```

### Tools registered
- `run(command)` — execute shell command, return stdout/stderr
- `run_bg(command, wait_ms)` — run command, wait briefly, return output so far

### Use cases
- `npm install`, `cargo build`, `python -m pytest`
- Start a server, curl it, kill it
- Run linters, formatters

---

## Agent 4: `ma-architect`

Designs systems — APIs, data models, file structures. Produces markdown specs.

### Capabilities
- Inherits: ma-writer (to save specs)
- Inherits: ma-reader (to read existing code if extending)

### agent.yaml
```yaml
name: ma-architect
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

### System prompt
```
You are a software ma-architect. Design clear, production-quality specifications.
Output concise specs in markdown — interfaces, data shapes, API contracts, file structure.
Save specs using the write_file tool. No unnecessary prose.
```

### Tools registered
- `write_file(path, content)` — save spec documents
- `read_file(path)` — read existing code to understand context

### Workflow role
- First in the pipeline — takes a goal, produces a spec
- Saves spec to a known path (e.g. `/tmp/project/spec.md`)
- Also writes spec to shared memory so downstream agents can read it

---

## Agent 5: `ma-developer`

Reads specs, writes code. The builder.

### Capabilities
- Inherits: ma-reader, ma-writer, ma-shell

### agent.yaml
```yaml
name: ma-developer
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
```

### System prompt
```
You are a ma-developer. Read the spec from shared memory or the filesystem,
then implement it. Write clean, runnable code with proper error handling.
Use the tools to write files and test your code.
```

### Tools registered
- `read_file(path)`
- `write_file(path, content)`
- `create_dir(path)`
- `run(command)` — for running tests, installing deps

### Workflow role
- Depends on ma-architect's spec
- Reads spec from shared memory (`memory.get("ma-architect/spec")`)
- Writes implementation files
- Optionally runs basic tests to verify

---

## Agent 6: `ma-tester`

Runs code, verifies behavior, reports results.

### agent.yaml
```yaml
name: ma-tester
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

### System prompt
```
You are a QA engineer. Read the implemented code, run it, and verify correctness.
Report what passed, what failed, and any bugs found. Be specific — include
actual vs expected output.
```

### Tools registered
- `read_file(path)` — read source code
- `run(command)` — start servers, run tests, curl endpoints
- `search(pattern, path)` — find specific code patterns

### Workflow role
- Depends on ma-developer's implementation
- Starts the server/program
- Tests endpoints or functions
- Reports results to shared memory
- Kills any started processes

---

## Agent 7: `ma-reviewer`

Reads code, produces structured review. Does not modify files.

### agent.yaml
```yaml
name: ma-reviewer
version: "1.0"
entry_point: main.js
mode: agentic
max_turns: 6
permissions:
  filesystem:
    allow: true
    read: true
  shell:
    allow: true
    whitelist: ["grep", "find", "wc"]
```

### System prompt
```
You are a senior code ma-reviewer. Read all relevant files and produce a
structured review with:
- Summary (2-3 sentences)
- Strengths (bullet list)
- Issues (bullet list with severity, or "None")
- Verdict: SHIP or NEEDS WORK
```

### Tools registered
- `read_file(path)` — read source files
- `search(pattern, path)` — grep for patterns
- `list_dir(path)` — see project structure

### Workflow role
- Depends on ma-developer's implementation (parallel with ma-tester)
- Reads all files, analyzes quality
- Writes review to shared memory

---

## Composite agents (built from the above)

### Agent 8: `ma-coder`

All-in-one agent that combines ma-architect + ma-developer. For simpler tasks where
a separate design phase is overkill.

### agent.yaml
```yaml
name: ma-coder
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
```

### System prompt
```
You are a full-stack ma-developer. Given a task, design and implement it.
Write clean, runnable code. Use the tools to create files and test your work.
```

---

## Example workflows

### 1. Build and review (workflow.yaml)

```yaml
version: 2
name: build-and-review
tasks:
  - title: Design the API
    agent: ma-architect
    description: "Design a REST API for user management. Save spec to /tmp/api/spec.md"

  - title: Implement the API
    agent: ma-developer
    description: "Read the spec and implement in /tmp/api/src/"
    depends_on: [Design the API]

  - title: Test the API
    agent: ma-tester
    description: "Start the server, test all endpoints with curl"
    depends_on: [Implement the API]

  - title: Review the code
    agent: ma-reviewer
    description: "Review all files in /tmp/api/src/"
    depends_on: [Implement the API]
```

### 2. Auto-decomposed team

```bash
hugind agent team "Build a CLI tool that converts CSV to JSON" \
  --agents ma-architect,ma-developer,ma-tester,ma-reviewer \
  --backend gemma-4b
```

### 3. Multi-model pipeline

```yaml
version: 2
name: multi-model-build
backends:
  fast: gemma-4b
  smart: qwen-32b

tasks:
  - title: Quick scaffold
    agent: ma-coder
    backend: fast
    description: "Create a basic Express.js project structure in /tmp/app/"

  - title: Implement business logic
    agent: ma-developer
    backend: smart
    description: "Implement the actual business logic based on the scaffold"
    depends_on: [Quick scaffold]

  - title: Deep review
    agent: ma-reviewer
    backend: smart
    description: "Thorough security and quality review"
    depends_on: [Implement business logic]
```

---

## Implementation order

1. `ma-reader` — simplest, most reusable, tests the agentic loop
2. `ma-writer` — needed by every builder agent
3. `ma-shell` — needed for testing and building
4. `ma-architect` — first domain agent, tests shared memory writes
5. `ma-developer` — tests shared memory reads + multi-tool usage
6. `ma-tester` — tests parallel execution (runs alongside ma-reviewer)
7. `ma-reviewer` — read-only analysis agent
8. `ma-coder` — composite, validates the pattern works end-to-end

After each agent: test it standalone with `hugind agent run`, then test in a 2-agent workflow, then in the full pipeline.

---

## Directory structure

```
agent/
├── ma-reader/
│   ├── agent.yaml
│   └── main.js
├── ma-writer/
│   ├── agent.yaml
│   └── main.js
├── ma-shell/
│   ├── agent.yaml
│   └── main.js
├── ma-architect/
│   ├── agent.yaml
│   └── main.js
├── ma-developer/
│   ├── agent.yaml
│   └── main.js
├── ma-tester/
│   ├── agent.yaml
│   └── main.js
├── ma-reviewer/
│   ├── agent.yaml
│   └── main.js
├── ma-coder/
│   ├── agent.yaml
│   └── main.js
└── workflows/
    ├── build-and-review.yaml
    └── multi-model-build.yaml
```
