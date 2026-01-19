# Agent Development Guide

This guide explains how to build and run Hugind agents locally, including the
manifest format, permissions, and available capabilities.

## Quick Start

1. Create a directory with `agent.yaml` and an entry point (default `main.dart`).
2. Start a server config (the agent calls the local server).
3. Run the agent by path for fast iteration.

Example:

```bash
hugind server start metal_unified
hugind agent run ./my-agent
```

## Agent Layout

```
my-agent/
  agent.yaml
  main.dart
```

The entry point can be any filename you set in `agent.yaml` (for example,
`main.dart` in the examples).

## Entry Point API

Your entry point should export a `main` function that receives a context map.
The context includes arguments and capability objects.

```dart
dynamic main(Map<String, dynamic> context) async {
  // Extract Helper Objects
  var sys = context['capabilities']['sys'];
  var llm = context['capabilities']['llm'];
  var net = context['capabilities']['net'];

  sys.print('Hello from agent');
  
  // Use LLM
  var answer = await llm.chat('Give me a short checklist for a release.');
  sys.print(answer);
  
  // Use Filesystem
  await sys.writeFile('checklist.txt', answer);
}
```

Available keys:

- `context['args']`: `List<String>` of CLI args passed after the agent name/path.
- `context['capabilities']['sys']`: System capabilities (IO, shell, tools).
- `context['capabilities']['llm']`: Local inference (chat).
- `context['capabilities']['net']`: Network access (fetch).

### SysCapability (`sys`)

- `void print(String msg)`: Print to stdout.
- `String readInput(String prompt)`: Read a line from stdin with a prompt.
- `Future<bool> confirm(String msg)`: Ask for Y/N confirmation.
- `Future<String> run(String executable, List<String> args, {String? workDir})`: Run a shell command.
- `Future<String> readFile(String path)`: Read text file.
- `Future<bool> writeFile(String path, String contents)`: Write text file.
- `Future<bool> exists(String path)`: Check if file/directory exists.
- `Future<bool> mkdir(String path, {bool recursive})`: Create directory.
- `Future<List<Map>> tools.list()`: List available MCP tools.
- `Future<dynamic> tools.call(String name, Map args)`: Execute an MCP tool.

### LlmCapability (`llm`)

- `Future<String> chat(String prompt, {String? system})`: Send a prompt to the configured model.

### NetworkCapability (`net`)

- `Future<String> fetch(String url)`: Perform an HTTP GET request (if allowed).

## Runtime Constraints

Agents run inside `dart_eval`, a secure Dart interpreter. This imposes strict limitations compared to standard Dart:

1.  **NO Imports**: You cannot use `import`. All allowed classes (`String`, `List`, `Map`, `Future`, `jsonDecode`, `jsonEncode`) are pre-loaded or available via capabilities.
2.  **No Stdout/Stdin**: Never use `print()`, `stdout`, or `stdin` directly. Use `sys.print()`, `sys.readInput()`.
3.  **No Nested Functions**: You cannot declare helper functions *inside* `main`. All logic must be inline or in top-level classes/functions (if the interpreter supports them, but inline is safest).
4.  **Strict Types**: Prefer explicit types (`Map<String, dynamic>`) over `var` where complex inference is needed.
5.  **No `!` Operator**: The unary bang operator (e.g. `!isValid`) may not be supported in all contexts. Use `isValid == false`.
6.  **Async/Await**: Always await futures (`sys.run`, `llm.chat`).

## Coding Patterns

### CLI Wrapper
To run shell commands safely and print output:

```dart
var cmd = "ls -la";
// Use sh -c to support pipes and wildcards
var result = await sys.run("sh", ["-c", cmd]);
sys.print(result);
```

### Infinite Loop Agent
For agents that run until told to stop (like a chat bot):

```dart
while (true) {
  var input = sys.readInput("You: ");
  if (input.trim().toLowerCase() == "exit") break;
  
  var response = await llm.chat(input);
  sys.print("Agent: " + response);
}
```

## Manifest (`agent.yaml`)

The `agent.yaml` file defines your agent's identity, permissions, and dependencies.

```yaml
# ==================================================================
# 🦅 HUGIND AGENT MANIFEST
# ==================================================================

name: "stock-analyst"
version: "1.0.0"
description: "Fetches live stock data and generates a PDF report."
entry_point: "main.dart"  # Default is main.dart

# ⚠️ COMPATIBILITY
hugind_version: ">=0.6.0"

# 🔗 BACKEND CONNECTION
# Defines which model server this agent talks to.
backend:
  url: "http://127.0.0.1:8080/v1"  # Optional override
  config: "gemma-2-9b-it"          # Provider config name in ~/.hugind/configs
  session:
    mode: "fresh"                  # stateless | fresh | resume
    id: "stock-analyst"            # Defaults to agent name if omitted

# 🛡️ PERMISSIONS (The Security Boundary)
permissions:

  # 🌐 NETWORK
  network:
    allow: true
    allowed_domains:
      - "api.stockdata.org"
      - "finance.yahoo.com"

  # 📂 FILESYSTEM
  filesystem:
    read: true
    write: true
    allowed_paths:
      - "$HOME/Downloads"
      - "./workspace"

  # 💻 SHELL / PROCESS EXECUTION
  shell:
    allow: true
    # Use EITHER whitelist OR blacklist
    whitelist:
      - "ls"
      - "date"
    # blacklist:
    #   - "rm"

# 🔌 DEPENDENCIES (MCP)
dependencies:
  mcp:
    - name: "postgres-client"
      required: true
    - name: "github"
      required: false

# 🔧 ENVIRONMENT VARIABLES
env:
  - name: "STOCK_API_KEY"
    required: true
    description: "API Key for stockdata.org"
```

### Backend Session Settings

Use `backend.session` to control the `X-Session-ID` header Hugind receives.

- `mode: stateless` omits the header entirely (default).
- `mode: fresh` sends a session ID and forces a fresh start on the first request.
- `mode: resume` sends a session ID and resumes prior context.
- `id` defaults to the agent name if omitted.

## Permissions Details

Permissions are denied by default. You must explicitly request them in `permissions`.

- **Filesystem**:
    - `allowed_paths`: List of directories the agent can access.
    - `read`: Boolean to enable `sys.readFile`, `sys.exists`.
    - `write`: Boolean to enable `sys.writeFile`, `sys.mkdir`.
    - Note: The agent's own directory is always allowed.

- **Network**:
    - `allow`: Boolean to enable `net.fetch`.
    - `allowed_domains`: Whitelist of domains (subdomains included).

- **Shell**:
    - `allow`: Boolean to enable `sys.run`.
    - `whitelist`: List of allowed executables.
    - `blacklist`: List of blocked executables.

## MCP Tools

Agents can use Model Context Protocol (MCP) tools provided by external servers.
Prerequisite: The user must have the MCP server configured in their `settings.yml`.

In `agent.yaml`, declare the dependency:

```yaml
dependencies:
  mcp:
    - name: "filesystem"
      required: true
```

In Dart:

```dart
var tools = await sys.tools.list();
var result = await sys.tools.call('filesystem_read_file', {'path': 'README.md'});
```

## Environment Variables

You can enforce required environment variables for your agent to run (e.g., API keys).

```yaml
env:
  - name: "API_KEY"
    required: true
```

The runtime will check for these variables in the user's environment before starting the agent.
