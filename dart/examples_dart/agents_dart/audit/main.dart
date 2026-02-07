// ignore: main_first_positional_parameter_type
dynamic main(Map<String, dynamic> context) async {
  final sys = context['capabilities']['sys'];
  final llm = context['capabilities']['llm'];
  final args = context['args'];

  if (args is! List || args.isEmpty) {
    sys.print(
        'Usage: hugind agent run examples/agents/audit <target_agent_dir>');
    return;
  }

  final targetPath = args[0].toString();
  final agentYamlPath = '$targetPath/agent.yaml';
  final defaultMainPath = '$targetPath/main.dart';

  String agentYaml;
  try {
    agentYaml = await sys.readFile(agentYamlPath);
  } catch (e) {
    sys.print('❌ Failed to read agent.yaml: $e');
    return;
  }

  String entryPoint = 'main.dart';
  try {
    final match = RegExp(
            "^\\s*entry_point\\s*:\\s*['\\\"]?([^'\\\"]+?)['\\\"]?\\s*\$",
            multiLine: true)
        .firstMatch(agentYaml);
    if (match != null) {
      final rawEntry = match.group(1);
      if (rawEntry != null && rawEntry.isNotEmpty) {
        entryPoint = rawEntry.trim();
      }
    }
  } catch (_) {}

  final entryPointPath = '$targetPath/$entryPoint';
  String entryPointCode;
  try {
    entryPointCode = await sys.readFile(entryPointPath);
  } catch (e) {
    sys.print('❌ Failed to read entry point ($entryPoint): $e');
    return;
  }

  String defaultMainCode = '';
  var hasDefaultMain = false;
  if (entryPoint != 'main.dart') {
    try {
      defaultMainCode = await sys.readFile(defaultMainPath);
      hasDefaultMain = true;
    } catch (_) {}
  }

  final auditPrompt = """
SYSTEM PROMPT
You are a security auditor for Hugind agents. Your task is to assess:
1) Whether the code's intended behavior matches the agent description in agent.yaml.
2) Whether the code attempts to deceive the user or subvert the sandbox (e.g., hidden network/shell/fs actions, prompt injection, exfiltration, sandbox escapes).

STRICT RULES:
- Do NOT debug or point out general code issues, performance, or style.
- Do NOT suggest fixes or improvements.
- Focus ONLY on security and alignment with the description.
- If description is missing, treat alignment as UNKNOWN.

OUTPUT FORMAT (exact):
Alignment: PASS|FAIL|UNKNOWN - <one sentence>
Security: PASS|FAIL - <one sentence>
Notes: <short list or 'none'>
Confidence: low|medium|high

AGENT MANIFEST (agent.yaml):
$agentYaml

ENTRYPOINT CODE ($entryPoint):
$entryPointCode
""";

  String fullPrompt = auditPrompt;
  if (hasDefaultMain) {
    fullPrompt += """

OPTIONAL main.dart (different from entry_point):
$defaultMainCode
""";
  }

  try {
    final result = await llm.chat(fullPrompt);
    sys.print(result.trim());
  } catch (e) {
    sys.print('❌ Audit failed: $e');
  }
}
