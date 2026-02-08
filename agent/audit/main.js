function usage() {
  print("Usage: hugind agent run agent/audit <target_agent_dir>");
}

function readFile(path) {
  return fs.read_text(path);
}

function resolveTargetPath(raw) {
  const input = String(raw || "").replace(/\/+$/, "");
  if (!input) return "";
  if (input.startsWith("/")) return input;

  // Try as-is (relative to agent root)
  if (fs.exists(input)) return input;

  const cwd = fs.cwd();
  const parts = String(cwd).split("/").filter(Boolean);
  if (parts.length >= 2) {
    const repoRoot = `/${parts.slice(0, parts.length - 2).join("/")}`;
    const candidate = `${repoRoot}/${input}`;
    if (fs.exists(candidate)) return candidate;
  }

  return input;
}

function extractEntryPoint(agentYaml) {
  const re = /^\s*entry_point\s*:\s*['"]?([^'"]+?)['"]?\s*$/m;
  const match = re.exec(agentYaml);
  if (match && match[1]) return String(match[1]).trim();
  return "main.js";
}

function buildPrompt(agentYaml, entryPoint, entryPointCode, defaultMainCode) {
  const base = [
    "SYSTEM PROMPT",
    "You are a security auditor for Hugind agents. Your task is to assess:",
    "1) Whether the code's intended behavior matches the agent description in agent.yaml.",
    "2) Whether the code attempts to deceive the user or subvert the sandbox (e.g., hidden network/shell/fs actions, prompt injection, exfiltration, sandbox escapes).",
    "",
    "STRICT RULES:",
    "- Do NOT debug or point out general code issues, performance, or style.",
    "- Do NOT suggest fixes or improvements.",
    "- Focus ONLY on security and alignment with the description.",
    "- If description is missing, treat alignment as UNKNOWN.",
    "",
    "OUTPUT FORMAT (exact):",
    "Alignment: PASS|FAIL|UNKNOWN - <one sentence>",
    "Security: PASS|FAIL - <one sentence>",
    "Notes: <short list or 'none'>",
    "Confidence: low|medium|high",
    "",
    "AGENT MANIFEST (agent.yaml):",
    agentYaml,
    "",
    `ENTRYPOINT CODE (${entryPoint}):`,
    entryPointCode
  ].join("\n");

  if (defaultMainCode) {
    return [
      base,
      "",
      "OPTIONAL main.js (different from entry_point):",
      defaultMainCode
    ].join("\n");
  }

  return base;
}

export default async function main(input) {
  const args = (input && Array.isArray(input.args)) ? input.args : [];
  if (!args.length) {
    usage();
    return;
  }

  const rawArg = String(args[0] || "");
  const targetPath = resolveTargetPath(rawArg);
  const agentYamlPath = `${targetPath}/agent.yaml`;
  const defaultMainPath = `${targetPath}/main.js`;

  let agentYaml;
  try {
    agentYaml = readFile(agentYamlPath);
  } catch (e) {
    print(`❌ Failed to read agent.yaml: ${String(e)}`);
    return;
  }

  const entryPoint = extractEntryPoint(agentYaml);
  const entryPointPath = `${targetPath}/${entryPoint}`;

  let entryPointCode;
  try {
    entryPointCode = readFile(entryPointPath);
  } catch (e) {
    print(`❌ Failed to read entry point (${entryPoint}): ${String(e)}`);
    return;
  }

  let defaultMainCode = "";
  if (entryPoint !== "main.js") {
    try {
      defaultMainCode = readFile(defaultMainPath);
    } catch (_) {
      defaultMainCode = "";
    }
  }

  const prompt = buildPrompt(agentYaml, entryPoint, entryPointCode, defaultMainCode);

  try {
    const result = await llm.chat(prompt);
    print(String(result).trim());
  } catch (e) {
    print(`❌ Audit failed: ${String(e)}`);
  }
}
