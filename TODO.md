# Hugind TODO

Based on the comparison between `README.md` claims and the current codebase state.

## 🔴 Missing Features (Promised in README)

### 1. Model Context Protocol (MCP) Client
**Status**: ❌ Missing
**Description**: The `README.md` and `agent.yaml` spec claim native support for MCP (connecting agents to external tools via standard servers).
**Gap**:
- `lib/agent/capabilities.dart` does not implement `sys.tools` or any MCP connection logic.
- No `mcp/` directory in `lib/`.
- `agent.yaml` dependencies (e.g., `- name: "filesystem"`) are currently ignored by the runtime.
**Action**:
- Implement an MCP Client in Dart.
- Add `sys.tools.list()` and `sys.tools.call()` capabilities to the sandbox.
- Implement process management to spawn/connect to MCP servers defined in user config.

## 🟡 Code Quality & Enhancements

### 1. Agent "Hot" Reload
**Status**: ⚠️ Partial
**Description**: Agents run as one-off scripts.
**Action**: Implement a watcher or persistent runner to allow long-running agents that verify changes (e.g., for `stock-analyst` monitoring).

### 2. Multi-Tenancy Tests
**Status**: ⚠️ Unverified in Tests
**Description**: `LlamaEngine` implements time-slicing and eviction, but there are no automated load tests ensuring this holds up under "100+ concurrent user sessions" as claimed.
**Action**: Add a stress-test script to `test/`.

## 🟢 Verified Features (Completed)

- [x] **3-Tier Memory System** (VRAM/RAM/Disk) - Implemented in `LlamaEngine`.
- [x] **Smart CLI Wizard** - Implemented in `ConfigCommand`.
- [x] **Vision Support** - Implemented in `ChatHandler` > `_parseContent`.
- [x] **Agent Sandbox** - Implemented in `AgentSandbox` & `dart_eval`.
- [x] **Native Performance** - Presets (Metal/CUDA) logic exists in `ConfigCommand`.
