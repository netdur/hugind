// ignore: main_first_positional_parameter_type
dynamic main(Map<String, dynamic> context) async {
  final sys = context['capabilities']['sys'];
  final llm = context['capabilities']['llm'];
  final args = context['args'];

  // --- 1. Validation & Setup ---
  if (args is! List || args.length < 2) {
    sys.print('Usage: hugind agent run builder <init|dev> <target_path>');
    return;
  }

  final command = args[0].toString();
  final targetPath = args[1].toString();
  final mainPath = '$targetPath/main.dart';
  final agentYamlPath = '$targetPath/agent.yaml';
  String userReq = "";
  String generatedAgentYaml = "";

  // --- 2. Handle Commands ---
  if (command == 'init') {
    sys.print('🏗️  Scaffolding new agent at: $targetPath');
    await sys.mkdir(targetPath);

    // We write a permissive agent.yaml fallback if generation fails
    final defaultYaml = """
name: "generated_agent"
version: "0.1.0"
entry_point: "main.dart"
backend: "qwen3vl-8b"
permissions:
  shell: { allow: true }
""";
    await sys.writeFile(agentYamlPath, defaultYaml);
    userReq = sys.readInput('What should this agent do? ');
  } else {
    sys.print('🛠️  Modifying agent at: $targetPath');
    userReq = sys.readInput('Describe the agent functionality: ');
  }

  sys.print('🧩 Generating agent.yaml...');

  // --- 3. AGENT.YAML GENERATION (Spec-first) ---
  String exampleAgentYaml = "";
  try {
    exampleAgentYaml = await sys.readFile('examples/agents/agent.yaml');
  } catch (e) {
    sys.print('⚠️  Could not read examples/agents/agent.yaml: $e');
  }

  final exampleYamlForPrompt = exampleAgentYaml == ""
      ? "(missing example; use reasonable defaults)"
      : exampleAgentYaml;

  final yamlInstruction = """
SYSTEM PROMPT
You are an expert Hugind agent spec author.
Generate a valid agent.yaml for the user request using the example below as the schema guide.

Rules:
- Output ONLY YAML (no code fences, no commentary).
- Use entry_point: "main.dart".
- Keep permissions least-privilege: request only what the agent needs.
- Include dependencies->mcp and env only if needed by the request.
- If backend is needed, use a simple model string (e.g. `backend: "qwen3vl-8b"`). Do NOT use a nested map like `backend: { recommended: ... }`.
- Default to `backend: "qwen3vl-8b"` unless the user request requires another model.

EXAMPLE AGENT.YAML:
$exampleYamlForPrompt

USER REQUEST:
$userReq

OUTPUT:
Return ONLY the YAML.
""";

  generatedAgentYaml = await llm.chat(yamlInstruction);
  generatedAgentYaml = generatedAgentYaml.trim();

  final yamlFence = RegExp(r'```(?:yaml)?\s*([\s\S]*?)```');
  final yamlMatch = yamlFence.firstMatch(generatedAgentYaml);
  if (yamlMatch != null && yamlMatch.groupCount >= 1) {
    generatedAgentYaml = yamlMatch.group(1)!.trim();
  }

  if (generatedAgentYaml.isEmpty) {
    sys.print('⚠️  YAML generation returned empty output. Keeping existing YAML.');
  } else {
    try {
      await sys.writeFile(agentYamlPath, generatedAgentYaml);
      sys.print("✅ Wrote agent.yaml to $agentYamlPath");
    } catch (e) {
      sys.print("❌ Failed to write agent.yaml: $e");
    }
  }

  sys.print('🤖 Generating code...');

  // --- 4. THE TECHNICAL SPECIFICATION PROMPT ---
  // This helps the model act like a compiler for your specific runtime.
  final agentYamlForPrompt =
      generatedAgentYaml == "" ? "(agent.yaml not available)" : generatedAgentYaml;

  final systemInstruction = """
SYSTEM PROMPT
You are an expert Developer for the Hugind AI Runtime.
Your task is to write a Dart agent script that EXACTLY satisfies the User Request.

### 1. THE RUNTIME ENVIRONMENT
You are running inside `dart_eval`, a secure Dart interpreter.
- **NO IMPORTS:** You CANNOT import libraries (`dart:io`, `package:http` are BANNED).
- **ENTRY POINT:** Exactly `dynamic main(Map<String, dynamic> context) async`.
- **TYPES:** Use strict types (`String`, `int`, `List<String>`) where possible.

### 2. THE API (Capabilities)
You must extract capabilities from the `context` object.
Here is the EXACT API available to you:

```dart
// EXTRACT THESE FIRST
var sys = context['capabilities']['sys'];
var llm = context['capabilities']['llm'];
var net = context['capabilities']['net']; // Optional
```

**SysCapability (sys):**
- `void print(String msg)`: Output text to user.
- `void printMsg(String msg)`: Alias for print.
- `String readInput(String prompt)`: Get input from user.
- `Future<String> run(String cmd, List<String> args, {String? workDir})`: Run shell commands.
- `Future<bool> confirm(String msg)`: Ask for Yes/No confirmation (returns true/false).
- `Future<String> readFile(String path)`: Read a file.
- `Future<bool> writeFile(String path, String contents)`: Write a file (overwrite).
- `Future<bool> exists(String path)`: Check if a file or directory exists.
- `Future<bool> mkdir(String path)`: Create a directory (recursive).
- `sys.tools.list()`: List MCP tools (returns `Future<List<Map<String, dynamic>>>`).
- `sys.tools.call(String name, Map<String, dynamic> args)`: Call MCP tool.

**LlmCapability (llm):**
- `Future<String> chat(String prompt)`: Send text to AI, get text back.

**NetworkCapability (net):**
- `Future<String> fetch(String url)`: GET request to a URL.

### 3. CODING PATTERNS (Follow these!)

**Pattern A: The CLI Tool**
Use this when the user wants to execute commands.
1. Read input.
2. Ask LLM to convert input to a shell command (e.g., "ls -la").
3. Execute with `sh -c` so pipes/redirections work: `await sys.run('sh', ['-c', command])`.
4. Print the command.
5. Ask `sys.confirm("Run this?")`.
6. If true, run it and print the returned output; if result is null or empty, print a "no output" message.

**Pattern B: The Chat/Math Bot**
Use this for reasoning, math, or translation.
1. Loop `while(true)`.
2. Read input.
3. Pass input to `await llm.chat(...)`.
4. Print result.

### 4. CRITICAL RULES
1. **NEVER** write `import`.
2. **NEVER** use `stdin` or `stdout` directly. Use `sys`.
3. **ALWAYS** check for "exit" or "quit" in the loop to break (exact equality after trim/lowercase).
4. **ALWAYS** use `await` for `run`, `confirm`, `chat`, and `fetch`.
5. **NEVER** use the unary `!` operator (dart_eval restriction). Use explicit comparisons like `confirmed == false`.
7. **ALWAYS** pass `sys.run` an executable and args list. Prefer `sh -c` for user commands:
   - `var result = await sys.run('sh', ['-c', command + " 2>&1"]);`
   - `if (result == null || result.trim().isEmpty) { sys.print("Command produced no output."); } else { sys.print(result); }`
8. **NEVER** declare nested functions or helpers inside `main` (dart_eval restriction). Keep all logic inline.
9. **OUTPUT** only valid Dart code, no markdown, no prose.
10. **AVOID** `List.sublist(...)` (dart_eval limitation). Build lists with loops instead.
11. **STRING METHODS** like `trim`, `toLowerCase`, `startsWith`, `substring`, and `split` are supported; use them normally.
12. **EXIT CHECK** should be exact equality after trim/lowercase (do NOT use `startsWith` for exit checks).
13. **CONFIRM CHECK** must be written as:
    - `if (confirmed == false) { ... } else { ... }`

### 5. MANDATORY BOILERPLATE
Start your code EXACTLY like this:

```dart
dynamic main(Map<String, dynamic> context) async {
  var sys = context['capabilities']['sys'];
  var llm = context['capabilities']['llm'];
  var net = context['capabilities']['net'];
  
  sys.print("Agent initialized.");
  
  while (true) {
     // Your logic...
```

### 6. AGENT.YAML (Permissions/Dependencies Context)
This is the generated agent.yaml for this agent. Respect the permissions it declares.
$agentYamlForPrompt

### USER REQUEST:
$userReq

### OUTPUT:
Return ONLY the valid Dart code block.
""";

  // --- 5. Generate & Extract ---
  String generatedCode = await llm.chat(systemInstruction);
  generatedCode = generatedCode.trim();
  final rawOutput = generatedCode;
  sys.print("Raw Output:");
  sys.print(generatedCode);

  // Robust Extraction
  final codeFence = RegExp(r'```(?:dart)?\s*([\s\S]*?)```');
  {
    var cleaned = generatedCode.trim();
    final match = codeFence.firstMatch(cleaned);
    if (match != null && match.groupCount >= 1) {
      cleaned = match.group(1)!.trim();
    }
    if (cleaned.contains('```')) {
      final parts = cleaned.split('```');
      for (var part in parts) {
        if (part.contains('dynamic main') || part.contains('void main')) {
          if (part.startsWith('dart')) {
            cleaned = part.substring(4).trim();
          } else {
            cleaned = part.trim();
          }
          break;
        }
      }
    }
    generatedCode = cleaned;
  }

  // --- 6. Sanitize ---
  // Fix imports using regex to catch various quoting styles
  // generatedCode = generatedCode.replaceAll(RegExp(r"import\s+['\"].*?['\"];"), "");

  // Fix entry point signature if model messed it up
  if (generatedCode.startsWith("void main")) {
    generatedCode = generatedCode.replaceFirst("void main", "dynamic main");
  }

  // --- 7. Verify ---
  final hasEntrypoint = generatedCode.contains('main(Map') ||
      generatedCode.contains('dynamic main') ||
      generatedCode.contains('void main');
  if (!hasEntrypoint) {
    sys.print("❌ Error: Valid code block not found.");
    sys.print("Raw Output: " + generatedCode);
    generatedCode = rawOutput;
  }

  // --- 8. Write to Disk (Force Overwrite) ---
  sys.print("💾 Writing to disk...");

  // B. Write new content
  try {
    await sys.writeFile(mainPath, generatedCode);

    // Check if write succeeded by checking existence
    bool exists = await sys.exists(mainPath);
    if (exists == false) {
      sys.print("❌ Write Failed.");
    } else {
      sys.print("✅ Successfully wrote to $mainPath");
      sys.print("------------------");
      sys.print(generatedCode);
      sys.print("------------------");
    }
  } catch (e) {
    sys.print("❌ Write Error: $e");
  }
}
