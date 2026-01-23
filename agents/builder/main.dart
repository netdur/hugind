// ignore: main_first_positional_parameter_type
dynamic main(Map<String, dynamic> context) async {
  final sys = context['capabilities']['sys'];
  final llm = context['capabilities']['llm'];
  final args = context['args'];

  // --- 1. Validation & Setup ---
  // Note: 'is!' works in dart_eval, but unary '!' on dynamic bools does not.
  if (args is! List || args.isEmpty) {
    sys.print('Usage: hugind agent run builder <target_path>');
    return;
  }

  final targetPath = args[0].toString();
  final mainPath = '$targetPath/main.dart';
  final agentYamlPath = '$targetPath/agent.yaml';
  String userReq = "";
  String generatedAgentYaml = "";

  // --- 2. Scaffold ---
  sys.print('🏗️  Scaffolding agent at: $targetPath');
  await sys.mkdir(targetPath);
  
  userReq = sys.readInput('Describe the agent functionality: ');

  sys.print('🧩 Generating configuration...');

  // --- 3. AGENT.YAML GENERATION ---
  final fullTemplate = """
name: "agent-name"
version: "0.1.0"
entry_point: "main.dart"
backend: "qwen3vl-8b"
# Alternative backend map form:
# backend:
#   url: "http://127.0.0.1:8080/v1"
#   config: "metal_unified"   # provider config name
#   model: "qwen3vl-8b"        # optional when using url
#   session:
#     mode: "fresh"            # stateless | fresh | resume
#     id: "agent-name"         # optional; defaults to agent name
permissions:
  network:
    allow: false
    allowed_domains: [] 
  filesystem:
    read: false
    write: false
    allowed_paths: []
  shell:
    allow: false
    whitelist: []
dependencies:
  mcp: []
  # Example:
  # mcp:
  #   - name: "github"
  #     required: false
env: []
""";

  final yamlInstruction = """
SYSTEM PROMPT
You are an expert Hugind Configuration Architect.
Generate a SECURE `agent.yaml` based on the User Request.

### INSTRUCTIONS
1. **Network**: Enable `allow: true` ONLY if fetching URLs. Add domains to `allowed_domains`.
2. **Filesystem**: Enable `read`/`write` ONLY if needed.
3. **Shell**: Enable `allow: true` ONLY if running commands. Populate `whitelist` with specific commands (e.g. `["ls", "git"]`).
4. **Shell policy**: Use EITHER `whitelist` OR `blacklist`, never both.
5. **Dependencies**: If user needs external tools (Postgres, Git, Brave), add them to `dependencies.mcp` with `name` and optional `required`.

### USER REQUEST
$userReq

### SCHEMA
$fullTemplate

### OUTPUT
Return ONLY valid YAML.
""";

  generatedAgentYaml = await llm.chat(yamlInstruction);
  generatedAgentYaml = generatedAgentYaml.trim();

  // Extract YAML
  final yamlFence = RegExp(r'```(?:yaml)?\s*([\s\S]*?)```');
  final yamlMatch = yamlFence.firstMatch(generatedAgentYaml);
  if (yamlMatch != null && yamlMatch.groupCount >= 1) {
    generatedAgentYaml = yamlMatch.group(1)!.trim();
  }
  
  // FIX: Unary '!' is forbidden in dart_eval for dynamic types.
  // We use '== false' instead.
  if (generatedAgentYaml.contains("backend:") == false) {
    generatedAgentYaml += '\nbackend: "qwen3vl-8b"';
  }

  try {
    await sys.writeFile(agentYamlPath, generatedAgentYaml);
    sys.print("✅ Wrote agent.yaml");
  } catch (e) {
    sys.print("❌ Failed to write agent.yaml: $e");
    return;
  }

  sys.print('🤖 Generating code logic...');

  // --- 4. THE TECHNICAL SPECIFICATION PROMPT ---
  
  // We read the YAML back to provide context
  final agentYamlContext = generatedAgentYaml; 
  String existingMain = "(New file)";
  try {
    final fileExists = await sys.exists(mainPath);
    if (fileExists) {
      existingMain = await sys.readFile(mainPath);
    }
  } catch (e) {}

  // This API definition matches your Sandbox Bridge exactly
  final apiDefinition = """
class SysCapability {
  // File System
  Future<String> readFile(String path);
  Future<bool> writeFile(String path, String content);
  Future<bool> exists(String path);
  Future<bool> mkdir(String path);
  
  // Interaction
  void print(String msg);
  String readInput(String prompt); // NOTE: Synchronous! Do not await.
  Future<bool> confirm(String msg);
  
  // Process
  Future<String> run(String executable, List<String> args, [String? workDir]);
  
  // Tools (MCP)
  final AgentToolsCapability tools;
}

class AgentToolsCapability {
  Future<List<Map<String, dynamic>>> list();
  Future<dynamic> call(String name, Map<String, dynamic> args);
}

class LlmCapability {
  Future<String> chat(String prompt);
}

class NetworkCapability {
  Future<String> fetch(String url);
}
""";

  final systemInstruction = """
SYSTEM PROMPT
You are an expert Developer for the Hugind AI Runtime.
Write the `main.dart` script to satisfy the User Request and `agent.yaml`.

### 1. RUNTIME ENVIRONMENT (`dart_eval`)
- **NO IMPORTS:** You cannot use `import`.
- **ENTRY POINT:** `dynamic main(Map<String, dynamic> context) async`.
- **FATAL ERRORS:** 
  1. The `!` operator crashes the system (e.g., `!exists`). Use `exists == false`.
  2. `sys.readInput` is SYNCHRONOUS. Do NOT use `await`.
  3. `sys.run` requires a `List<String>` for args.

### 2. THE API
You have access to these exact classes via `context`.

```dart
$apiDefinition
```

### 3. IMPLEMENTATION PATTERNS

**Standard Initialization:**
```dart
// ignore: main_first_positional_parameter_type
dynamic main(Map<String, dynamic> context) async {
  var sys = context['capabilities']['sys'];
  var llm = context['capabilities']['llm'];
  var net = context['capabilities']['net'];
  
  sys.print("Agent started.");
  // ... loop ...
}
```

**Using Tools (MCP):**
If `agent.yaml` has MCP dependencies:
```dart
var tools = await sys.tools.list();
sys.print("Available tools: " + tools.toString());
// Calling a tool
var result = await sys.tools.call("tool_name", {"arg": "value"});
```

**Running Shell Commands:**
```dart
// Use 'sh -c' for complex commands or redirects
var output = await sys.run("sh", ["-c", "ls -la"], "/tmp");
```

### 4. CONTEXT
**agent.yaml:**
$agentYamlContext

**User Request:**
$userReq

### OUTPUT
Return ONLY valid Dart code.
""";

  // --- 5. Generate Code ---
  String generatedCode = await llm.chat(systemInstruction);
  generatedCode = generatedCode.trim();

  // Cleanup
  final codeFence = RegExp(r'```(?:dart)?\s*([\s\S]*?)```');
  final match = codeFence.firstMatch(generatedCode);
  if (match != null && match.groupCount >= 1) {
    generatedCode = match.group(1)!.trim();
  }

  // Sanitize imports
  if (generatedCode.contains('import ')) {
     generatedCode = generatedCode.replaceAll(RegExp(r"^import .*?;", multiLine: true), "");
  }
  // Sanitize main signature
  if (generatedCode.startsWith("void main")) {
    generatedCode = generatedCode.replaceFirst("void main", "dynamic main");
  }
  // Sanitize unary ! operator
  if (generatedCode.contains('if (!')) {
      generatedCode = generatedCode.replaceAll("if (!", "if (false == ");
  }

  // --- 6. Write Code ---
  sys.print("💾 Writing logic to $mainPath...");
  try {
    await sys.writeFile(mainPath, generatedCode);
    sys.print("✅ Success!");
    sys.print("------------------");
    sys.print(generatedCode);
    sys.print("------------------");
  } catch (e) {
    sys.print("❌ Write Error: $e");
  }
}
