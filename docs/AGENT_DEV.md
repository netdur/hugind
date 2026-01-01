# 🛠️ Hugind Agent Developer Guide

This guide will teach you how to build autonomous agents for the **Hugind** platform. An agent is essentially a lightweight Dart script that can interact with your local LLM server and your operating system.

---

## 🚀 Quick Start: Hello World

Let's create an agent that says hello and asks an LLM for a joke.

### 1. Create the Agent Directory
Agents live in `~/.hugind/agents/`. Create a folder for your new agent:

```bash
mkdir -p ~/.hugind/agents/joke-bot
```

### 2. Create the Manifest (`agent.yaml`)
Create `~/.hugind/agents/joke-bot/agent.yaml`. This file tells Hugind how to run your agent.

```yaml
name: "JokeBot"
version: "0.1.0"
description: "A friendly bot that tells jokes."
backend: "gemma-2b"  # Name of a config file in ~/.hugind/configs/
entry_point: "main.drt"
```

> **Note:** Ensure you have a server config named `gemma-2b.yml` (or whatever you specify) and that the server is running (`hugind server start gemma-2b`).

### 3. Write the Script (`main.drt`)
Create `~/.hugind/agents/joke-bot/main.drt`. This is the logic of your agent.

```dart
dynamic main(Map<String, dynamic> context) async {
  // 1. Get Capabilities
  var sys = context['capabilities']['sys'];
  var llm = context['capabilities']['llm'];
  var args = context['args'];

  sys.print("🤖 Beep Boop. I am JokeBot.");

  // 2. Interact with the User
  if (await sys.confirm("Do you want to hear a joke?")) {
    
    sys.print("Thinking...");
    
    // 3. Call the LLM
    var joke = await llm.chat("Tell me a short, clean programming joke.");
    
    sys.print("\n" + joke + "\n");
    
  } else {
    sys.print("Okay, maybe next time!");
  }
}
```

### 4. Run It!
```bash
hugind agent run joke-bot
```

---

## 📘 API Reference

Your script runs in a **Sandbox**. You don't have direct access to `dart:io`. Instead, you use the `capabilities` injected into the `context`.

### `SysCapability` (`context['capabilities']['sys']`)
Interact with the system and user.

*   **`void print(dynamic msg)`**
    *   Prints to the console.
*   **`Future<bool> confirm(String message)`**
    *   Asks the user a Yes/No question. Returns `true` for Yes.
*   **`Future<String> run(String executable, List<String> args, {String? workDir})`**
    *   Runs a shell command.
    *   **Security:** By default, agents can only access the current working directory.
    *   *Example:* `await sys.run('git', ['status']);`

### `LlmCapability` (`context['capabilities']['llm']`)
Interact with the local Inference Server.

*   **`Future<String> chat(String prompt)`**
    *   Sends a user message to the configured model and returns the response text.

---

## ⚡️ Tips & Tricks

### Using Arguments
You can pass arguments from the CLI to your agent.
```bash
hugind agent run my-agent arg1 arg2
```
Access them in your script:
```dart
var args = context['args']; // ["arg1", "arg2"]
if (args.isNotEmpty) {
  sys.print("Processing " + args[0]);
}
```

### Prompt Engineering
Since you are connecting to local models (which might be smaller, like 4B or 7B parameters), keep your prompts simple and direct.
*   ❌ "Construct a complex JSON object describing the file..."
*   ✅ "Read this file. List the main functions in it."

### Debugging
The sandbox doesn't support a debugger yet. Use `sys.print()` heavily to trace execution.

---

## ⚠️ Gotchas & Limitations

The agent logic runs in `dart_eval`, a Dart interpreter. It is **not** a full Dart environment.

1.  **No `import`**
    *   You cannot import standard libraries like `dart:io` or `dart:convert`. You must rely on the provided capabilities.
    *   Basic `dart:core` (String, int, List, Map, etc.) is available.

2.  **Type Safety**
    *   The bridge between the Interpreter and the Host is sensitive.
    *   Avoid complex generics in function calls if possible.
    *   `context['args']` is a `List`, but treat it carefully.

3.  **Recursion**
    *   Deep recursion might overflow the interpreter stack more quickly than native Dart.

4.  **Async/Await**
    *   Always use `await` when calling `sys` or `llm` methods that return Futures. Forgetting `await` will result in unexpected behavior or race conditions.

---

## ❓ FAQ

**Q: Can I install pub packages in my agent?**
A: No. Agents are single-file scripts designed to be portable and secure. If you need complex logic, consider building a CLI tool that Hugind calls, or ask for a new Capability to be added to Hugind Core.

**Q: Where are the logs?**
A: `hugind agent run` outputs directly to your stdout. Server logs depend on how you started `hugind server`.

**Q: How do I change the model?**
A: Edit the `backend` field in your `agent.yaml` to point to a different server config file.
