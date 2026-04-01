# Multi-Agent Orchestration Plan

Goal: extend Hugind to support coordinated multi-agent workflows with shared state, dependency-driven execution, parallel agents, and an agentic conversation loop — all running against local Hugind servers (potentially multiple models on different ports).

---

## Phase 1: Shared Memory

Agents in a workflow need to see what other agents produced. Currently each agent is fully isolated.

### Design

Add a `SharedMemory` struct to `core/orchestrator/`:

```
SharedMemory {
    store: HashMap<String, Value>   // namespaced as "agent_name/key"
}
```

- Write: `memory.set("researcher", "findings", json!(...))`
- Read: `memory.get("researcher/findings") -> Option<Value>`
- Summary: `memory.summary() -> String` (markdown grouped by agent)
- Injected into agent prompts as context by the orchestrator

### Runtime Integration

Expose to JS agents as `memory.get(key)`, `memory.set(key, value)`, `memory.list()`.
Expose to WASM agents as hostcalls: `hugind.memory_get`, `hugind.memory_set`.

The SharedMemory instance lives in the orchestrator and is passed into each agent run. It persists across all steps in a workflow.

### Files to create/modify

- `src/core/orchestrator/memory.rs` — SharedMemory struct
- `src/core/js/capabilities/memory.rs` — JS bindings
- `src/core/wasm/runtime.rs` — WASM hostcalls
- `src/core/orchestrator.rs` — pass memory through workflow

---

## Phase 2: Task Queue with Dependency DAG

Replace the flat `Vec<WorkflowStep>` with a proper task graph.

### Design

```
Task {
    id: String,
    title: String,
    description: String,
    assignee: Option<String>,     // agent name
    depends_on: Vec<String>,      // task IDs or titles
    status: TaskStatus,           // Pending | Blocked | InProgress | Completed | Failed
    result: Option<Value>,
}

TaskQueue {
    tasks: HashMap<String, Task>,
    event_tx: broadcast::Sender<TaskEvent>,
}
```

Key behaviors:
- Tasks start as `Blocked` if they have unmet dependencies, `Pending` otherwise
- When a task completes, scan all `Blocked` tasks — promote to `Pending` if all deps satisfied
- When a task fails, cascade failure to all transitive dependents
- `next_ready() -> Vec<&Task>` returns all `Pending` tasks (can run in parallel)

### Workflow YAML (v2)

```yaml
version: 2
name: build-api
tasks:
  - title: Design API spec
    agent: architect
    description: Design a REST API for user management

  - title: Implement API
    agent: developer
    description: Implement the API based on the spec
    depends_on: [Design API spec]

  - title: Write tests
    agent: tester
    description: Write integration tests
    depends_on: [Implement API]

  - title: Review code
    agent: reviewer
    description: Review implementation and tests
    depends_on: [Implement API, Write tests]
```

Backward compat: `version: 1` workflows (flat steps) still work with the current sequential runner.

### Files to create/modify

- `src/core/orchestrator/task.rs` — Task, TaskStatus, TaskQueue
- `src/core/config/workflow.rs` — WorkflowConfig v2 with tasks + depends_on
- `src/core/orchestrator.rs` — new `run_workflow_v2` that drives the task queue

---

## Phase 3: Parallel Agent Execution

Run independent tasks concurrently.

### Design

The orchestrator loop becomes:

```
loop {
    let ready = queue.next_ready();
    if ready.is_empty() && queue.has_in_progress() {
        // Wait for any running task to finish
        wait_for_completion().await;
        continue;
    }
    if ready.is_empty() {
        break; // all done or deadlocked
    }

    // Spawn all ready tasks concurrently
    for task in ready {
        queue.set_status(task.id, InProgress);
        tokio::spawn(run_agent_task(task, memory, ...));
    }
}
```

Concurrency control via a semaphore (configurable, default 4). Each agent task acquires a permit before running.

### Considerations

- SharedMemory needs `Arc<RwLock<...>>` for concurrent access
- Logger needs to be thread-safe (already is via `RunLogger`)
- Each agent gets its own runtime instance (already the case)
- Task results written to SharedMemory under `task.assignee/task.title`

### Files to modify

- `src/core/orchestrator.rs` — parallel execution loop
- `src/core/orchestrator/memory.rs` — wrap in Arc<RwLock>

---

## Phase 4: Inter-Agent Messaging

Allow agents to send messages to each other within a workflow.

### Design

```
MessageBus {
    messages: Vec<Message>,
}

Message {
    from: String,
    to: String,       // agent name or "*" for broadcast
    content: String,
    timestamp: Instant,
    read: bool,
}
```

Operations:
- `send(from, to, content)` — point-to-point
- `broadcast(from, content)` — to all except sender
- `get_messages(agent_name) -> Vec<Message>` — all messages for an agent
- `get_unread(agent_name) -> Vec<Message>` — unread only

### Runtime Integration

JS: `messaging.send(to, content)`, `messaging.receive()`, `messaging.broadcast(content)`
WASM: `hugind.msg_send`, `hugind.msg_receive`, `hugind.msg_broadcast`

### Prompt Injection

Before each agent runs, the orchestrator prepends any pending messages to the prompt:

```
## Messages from team members
- **architect**: The API spec is ready. See shared memory key "architect/api-spec".
- **coordinator** (broadcast): Priority change — focus on auth endpoints first.
```

### Files to create/modify

- `src/core/orchestrator/messaging.rs` — MessageBus
- `src/core/js/capabilities/messaging.rs` — JS bindings
- `src/core/wasm/runtime.rs` — WASM hostcalls
- `src/core/orchestrator.rs` — inject messages into agent prompts

---

## Phase 5: Agentic Conversation Loop

Currently agents call `llm.chat()` manually. Add a built-in loop that drives LLM → tool → LLM cycles automatically.

### Design

New execution mode in agent.yaml:

```yaml
entry_point: main.js
mode: agentic          # new field, default: "script"
max_turns: 10          # max LLM round-trips
```

When `mode: agentic`:
1. Agent code defines available tools (via a registration API)
2. Orchestrator sends the initial prompt to the LLM
3. If LLM responds with tool calls, orchestrator executes them and feeds results back
4. Loop until LLM responds with no tool calls or max_turns reached

This is similar to how open-multi-agent's `AgentRunner` works, but tools are the existing Hugind capabilities (fs, shell, net) plus any custom tools the agent defines.

### Tool Registration API (JS)

```js
register_tool({
  name: "search_docs",
  description: "Search project documentation",
  parameters: { query: { type: "string" } },
  execute: async (params) => {
    // custom logic
    return { result: "..." };
  }
});
```

### Files to create/modify

- `src/core/orchestrator/agentic.rs` — agentic loop implementation
- `src/core/js/capabilities/tools.rs` — tool registration API for JS
- `src/core/config/agent.rs` — add `mode` and `max_turns` fields

---

## Phase 6: Multi-Model Backend Support

Hugind already resolves backends per-agent via `agent.yaml`:

```yaml
backend:
  config: gemma-4b      # references ~/.hugind/configs/gemma-4b.yml → host:port
```

This already supports different models on different ports. What's needed:

### Workflow-Level Backend Mapping

Allow the workflow to declare which agents use which backends:

```yaml
version: 2
name: multi-model-pipeline
backends:
  fast: gemma-4b        # port 8080
  smart: qwen-32b       # port 8081

tasks:
  - title: Quick analysis
    agent: scanner
    backend: fast
    description: Scan the codebase for issues

  - title: Deep review
    agent: reviewer
    backend: smart
    description: Review the scanner's findings in depth
    depends_on: [Quick analysis]
```

The orchestrator overrides each agent's backend based on the workflow mapping before execution.

### Health Checks

Before starting a multi-model workflow, check all referenced backends are healthy. Fail fast with a clear message listing which servers are down.

### Files to modify

- `src/core/config/workflow.rs` — add `backends` map and per-task `backend` field
- `src/core/orchestrator.rs` — apply backend overrides, multi-server health check

---

## Phase 7: Coordinator Pattern

Auto-decompose a natural language goal into a task DAG using a temporary coordinator agent.

### Design

New CLI command:

```bash
hugind agent team "Build a REST API for user management" \
  --agents architect,developer,tester \
  --backend smart
```

Flow:
1. Create a temporary coordinator agent with a system prompt describing the team roster
2. Send the goal to the coordinator via the LLM
3. Coordinator responds with structured JSON: array of tasks with titles, descriptions, assignees, dependencies
4. Parse the JSON into TaskQueue
5. Execute using the Phase 2-3 machinery
6. After all tasks complete, send the coordinator a synthesis prompt with all results + shared memory
7. Return the coordinator's final summary

### Coordinator System Prompt Template

```
You are a project coordinator. Your team consists of:
{{#each agents}}
- {{name}}: {{description}}
{{/each}}

Decompose the following goal into a set of tasks. Each task should have:
- title (short, unique)
- description (what to do)
- assignee (one of the team member names)
- dependsOn (array of task titles this task depends on, or empty)

Respond with a JSON array of tasks. No other text.
```

### Files to create/modify

- `src/core/orchestrator/coordinator.rs` — coordinator agent logic
- `src/cli/agent.rs` — add `team` subcommand
- `src/cli/args.rs` — TeamCommand

---

## Phase 8: Streaming Events

Emit structured events during orchestration for real-time progress tracking.

### Design

```
OrchestratorEvent {
    timestamp: Instant,
    kind: EventKind,
}

EventKind:
    WorkflowStart { name }
    TaskReady { task_id, title, assignee }
    TaskStart { task_id, title, assignee }
    TaskComplete { task_id, title, result_preview }
    TaskFailed { task_id, title, error }
    AgentMessage { from, to, content }
    MemoryWrite { agent, key }
    WorkflowComplete { success, duration }
```

Delivered via a `tokio::sync::broadcast` channel. The CLI subscribes and prints progress. The stdio bridge forwards events as NDJSON.

### Files to create/modify

- `src/core/orchestrator/events.rs` — event types and channel
- `src/core/orchestrator.rs` — emit events at key points
- `src/cli/agent.rs` — subscribe and display progress
- `src/stdio/mod.rs` — forward events as NDJSON

---

## Phase 9: Dynamic Task Spawning

Allow agents to create sub-tasks at runtime.

### Design

Expose to agents:

```js
// JS
spawn_task({
  title: "Fix auth bug",
  description: "The auth endpoint returns 500 on invalid tokens",
  assignee: "developer",
  depends_on: []  // runs as soon as possible
});
```

The orchestrator watches for new tasks being added to the queue during execution and schedules them into the running workflow.

### Considerations

- New tasks can only depend on existing tasks (no forward references)
- The orchestrator loop already processes `next_ready()` each iteration, so dynamically added tasks are picked up naturally
- Need cycle detection when adding dependencies

### Files to modify

- `src/core/orchestrator/task.rs` — cycle detection on add
- `src/core/js/capabilities/tasks.rs` — JS spawn_task binding
- `src/core/wasm/runtime.rs` — WASM hostcall

---

## Implementation Order

```
Phase 1: Shared Memory              ← foundation, enables everything
Phase 2: Task Queue + DAG           ← replaces flat workflows
Phase 3: Parallel Execution         ← unlocks concurrency
Phase 4: Inter-Agent Messaging      ← communication layer
Phase 5: Agentic Loop               ← autonomous tool-use cycles
Phase 6: Multi-Model Backends       ← workflow-level backend mapping
Phase 7: Coordinator Pattern        ← auto-decomposition from goals
Phase 8: Streaming Events           ← observability
Phase 9: Dynamic Task Spawning      ← runtime flexibility
```

Each phase is independently useful and builds on the previous ones. Phases 1-3 are the critical path — they turn Hugind from a single-agent runner into a multi-agent orchestrator.
