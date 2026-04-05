use parking_lot::Mutex;
use serde_json::Value as JsonValue;
use std::sync::Arc;

/// A tool registered by the agent at setup time.
#[derive(Debug, Clone)]
pub struct AgentTool {
    pub name: String,
    pub description: String,
    pub parameters: JsonValue,
}

/// A parsed tool call from the LLM's text response.
#[derive(Debug, Clone)]
pub struct ParsedToolCall {
    pub name: String,
    pub args: JsonValue,
}

/// Registry of tools declared by the agent's JS entry point.
#[derive(Clone)]
pub struct ToolRegistry {
    tools: Arc<Mutex<Vec<AgentTool>>>,
    system_prompt: Arc<Mutex<Option<String>>>,
    max_turns: Arc<Mutex<Option<u32>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: Arc::new(Mutex::new(Vec::new())),
            system_prompt: Arc::new(Mutex::new(None)),
            max_turns: Arc::new(Mutex::new(None)),
        }
    }

    pub fn register(&self, tool: AgentTool) {
        self.tools.lock().push(tool);
    }

    pub fn set_system_prompt(&self, prompt: String) {
        *self.system_prompt.lock() = Some(prompt);
    }

    pub fn get_system_prompt(&self) -> Option<String> {
        self.system_prompt.lock().clone()
    }

    pub fn set_max_turns(&self, turns: u32) {
        *self.max_turns.lock() = Some(turns);
    }

    pub fn get_max_turns(&self) -> Option<u32> {
        *self.max_turns.lock()
    }

    pub fn tools(&self) -> Vec<AgentTool> {
        self.tools.lock().clone()
    }

    /// Build a compact text description of available tools for the system prompt.
    pub fn tools_prompt(&self) -> String {
        let tools = self.tools.lock();
        if tools.is_empty() {
            return String::new();
        }

        let mut out = String::from(
            "\n\nYou have tools. To use one: <tool_call>{\"name\":\"tool_name\",\"args\":{...}}</tool_call>\n\
             When done, respond without tool_call tags.\n\n",
        );

        for tool in tools.iter() {
            // Extract just the required property names for a compact signature
            let params_summary = if let Some(props) = tool.parameters.get("properties") {
                if let Some(obj) = props.as_object() {
                    let names: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
                    if names.is_empty() {
                        String::new()
                    } else {
                        format!("({})", names.join(", "))
                    }
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            out.push_str(&format!("- {}{}: {}\n", tool.name, params_summary, tool.description));
        }

        out
    }
}

/// Parse tool calls from LLM response text.
/// Looks for `<tool_call>{...}</tool_call>` blocks.
/// Also handles variant tag formats from models like Gemma (e.g. `<|tool_call>...<tool_call|>`).
/// Tolerates non-strict JSON (unquoted keys) which small LLMs often produce.
pub fn parse_tool_calls(text: &str) -> Vec<ParsedToolCall> {
    // Try standard format first
    let mut calls = parse_tool_calls_with_tags(text, "<tool_call>", "</tool_call>");

    // Fallback: Gemma-style <|tool_call>...<tool_call|> or <|tool_call|>...<|/tool_call|>
    if calls.is_empty() {
        calls = parse_tool_calls_with_tags(text, "<|tool_call>", "<tool_call|>");
    }
    if calls.is_empty() {
        calls = parse_tool_calls_with_tags(text, "<|tool_call|>", "<|/tool_call|>");
    }

    calls
}

fn parse_tool_calls_with_tags(text: &str, start_tag: &str, end_tag: &str) -> Vec<ParsedToolCall> {
    let mut calls = Vec::new();
    let mut search_from = 0;

    loop {
        let start = match text[search_from..].find(start_tag) {
            Some(pos) => search_from + pos + start_tag.len(),
            None => break,
        };

        let end = match text[start..].find(end_tag) {
            Some(pos) => start + pos,
            None => break,
        };

        let inner = text[start..end].trim();

        // Try standard JSON format: {"name":"tool","args":{...}}
        if let Some(call) = try_parse_json_tool_call(inner) {
            calls.push(call);
        }
        // Try call:name{...} format (Gemma-style): call:run{command: "ls"}
        else if let Some(call) = try_parse_call_colon_format(inner) {
            calls.push(call);
        }

        search_from = end + end_tag.len();
    }

    calls
}

/// Parse standard `{"name":"tool","args":{...}}` format.
fn try_parse_json_tool_call(json_str: &str) -> Option<ParsedToolCall> {
    let parsed = serde_json::from_str::<JsonValue>(json_str)
        .or_else(|_| {
            let fixed = fix_unquoted_keys(json_str);
            serde_json::from_str::<JsonValue>(&fixed)
        })
        .ok()?;

    let name = parsed.get("name").and_then(|n| n.as_str())?.to_string();
    let args = parsed.get("args").cloned()
        .unwrap_or(JsonValue::Object(Default::default()));

    if name.is_empty() { return None; }
    Some(ParsedToolCall { name, args })
}

/// Parse `call:tool_name{...}` format produced by some models (e.g. Gemma).
/// Example: `call:run{command: "ls /Applications"}`
fn try_parse_call_colon_format(inner: &str) -> Option<ParsedToolCall> {
    let rest = inner.strip_prefix("call:")?;

    // Find the opening brace
    let brace_pos = rest.find('{')?;
    let name = rest[..brace_pos].trim().to_string();
    if name.is_empty() { return None; }

    let json_str = rest[brace_pos..].trim();

    // The content inside braces is the args object directly (not wrapped in {"args":...})
    let args = serde_json::from_str::<JsonValue>(json_str)
        .or_else(|_| {
            let fixed = fix_unquoted_keys(json_str);
            serde_json::from_str::<JsonValue>(&fixed)
        })
        .unwrap_or(JsonValue::Object(Default::default()));

    Some(ParsedToolCall { name, args })
}

/// Fix common JSON issues from LLMs: unquoted keys, trailing commas.
fn fix_unquoted_keys(input: &str) -> String {
    let mut result = String::with_capacity(input.len() + 32);
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut prev_was_escape = false;

    while let Some(c) = chars.next() {
        if in_string {
            result.push(c);
            if c == '\\' && !prev_was_escape {
                prev_was_escape = true;
            } else {
                if c == '"' && !prev_was_escape {
                    in_string = false;
                }
                prev_was_escape = false;
            }
            continue;
        }

        match c {
            '"' => {
                in_string = true;
                result.push(c);
            }
            // Potential unquoted key: letter followed eventually by ':'
            c if c.is_alphabetic() || c == '_' => {
                let mut word = String::new();
                word.push(c);
                while let Some(&next) = chars.peek() {
                    if next.is_alphanumeric() || next == '_' {
                        word.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                // Check if followed by ':' (skip whitespace)
                let mut is_key = false;
                let mut skipped_ws = String::new();
                while let Some(&next) = chars.peek() {
                    if next == ' ' || next == '\t' {
                        skipped_ws.push(chars.next().unwrap());
                    } else if next == ':' {
                        is_key = true;
                        break;
                    } else {
                        break;
                    }
                }

                if is_key {
                    // It's an unquoted key — add quotes
                    result.push('"');
                    result.push_str(&word);
                    result.push('"');
                    result.push_str(&skipped_ws);
                } else {
                    // It's a bare value like true/false/null
                    result.push_str(&word);
                    result.push_str(&skipped_ws);
                }
            }
            // Remove trailing commas before } or ]
            ',' => {
                // Peek ahead past whitespace for } or ]
                let rest: String = chars.clone().collect();
                let trimmed = rest.trim_start();
                if trimmed.starts_with('}') || trimmed.starts_with(']') {
                    // Skip the trailing comma
                } else {
                    result.push(',');
                }
            }
            _ => result.push(c),
        }
    }

    result
}

/// Strip `<think>...</think>` blocks from LLM responses (used by thinking-enabled models).
/// Also handles unclosed `<think>` tags (strip from `<think>` to end of text).
pub fn strip_thinking(text: &str) -> String {
    let mut result = text.to_string();
    loop {
        if let Some(start) = result.find("<think>") {
            if let Some(end_offset) = result[start..].find("</think>") {
                result = format!(
                    "{}{}",
                    &result[..start],
                    &result[start + end_offset + "</think>".len()..]
                );
                continue;
            } else {
                // Unclosed <think> — strip from tag to end
                result = result[..start].to_string();
            }
        }
        break;
    }
    result.trim().to_string()
}

/// Extract the non-tool-call text from a response (the "thinking" or final answer).
pub fn strip_tool_calls(text: &str) -> String {
    let mut result = text.to_string();
    loop {
        let start_tag = "<tool_call>";
        let end_tag = "</tool_call>";
        if let Some(start) = result.find(start_tag) {
            if let Some(end) = result[start..].find(end_tag) {
                result = format!(
                    "{}{}",
                    &result[..start],
                    &result[start + end + end_tag.len()..]
                );
                continue;
            }
        }
        break;
    }
    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_single_tool_call() {
        let text = r#"Let me read the file.
<tool_call>{"name": "read_file", "args": {"path": "/tmp/foo.py"}}</tool_call>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].args["path"], "/tmp/foo.py");
    }

    #[test]
    fn parse_multiple_tool_calls() {
        let text = r#"I'll read two files.
<tool_call>{"name": "read_file", "args": {"path": "a.py"}}</tool_call>
<tool_call>{"name": "read_file", "args": {"path": "b.py"}}</tool_call>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 2);
    }

    #[test]
    fn parse_unquoted_keys() {
        let text = r#"<tool_call>{"name":"read_file","args":{path: "/tmp/foo.py"}}</tool_call>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].args["path"], "/tmp/foo.py");
    }

    #[test]
    fn parse_no_tool_calls() {
        let text = "Here is the final answer. No tools needed.";
        let calls = parse_tool_calls(text);
        assert!(calls.is_empty());
    }

    #[test]
    fn strip_tool_calls_preserves_text() {
        let text = r#"I'll check.
<tool_call>{"name": "read_file", "args": {"path": "x"}}</tool_call>
Done."#;
        let stripped = strip_tool_calls(text);
        assert!(stripped.contains("I'll check."));
        assert!(stripped.contains("Done."));
        assert!(!stripped.contains("tool_call"));
    }

    #[test]
    fn tools_prompt_format() {
        let reg = ToolRegistry::new();
        reg.register(AgentTool {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        });
        let prompt = reg.tools_prompt();
        assert!(prompt.contains("read_file(path)"));
        assert!(prompt.contains("<tool_call>"));
    }

    #[test]
    fn parse_gemma_pipe_tags() {
        let text = r#"<|tool_call>call:run{command: "ls /Applications"}<tool_call|>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "run");
        assert_eq!(calls[0].args["command"], "ls /Applications");
    }

    #[test]
    fn parse_gemma_call_colon_quoted() {
        let text = r#"<|tool_call>call:run{"command": "ls -la"}<tool_call|>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "run");
        assert_eq!(calls[0].args["command"], "ls -la");
    }

    #[test]
    fn parse_gemma_pipe_tags_v2() {
        let text = r#"<|tool_call|>call:read_file{"path": "/tmp/x"}<|/tool_call|>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].args["path"], "/tmp/x");
    }

    #[test]
    fn parse_standard_preferred_over_gemma() {
        // If standard tags exist, they should be found
        let text = r#"<tool_call>{"name":"run","args":{"command":"echo hi"}}</tool_call>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "run");
    }

    #[test]
    fn strip_thinking_closed() {
        let text = "<think>Let me think about this...</think>Here is the answer.";
        assert_eq!(strip_thinking(text), "Here is the answer.");
    }

    #[test]
    fn strip_thinking_unclosed() {
        let text = "Some text\n<think>I'm still thinking...";
        assert_eq!(strip_thinking(text), "Some text");
    }

    #[test]
    fn strip_thinking_with_tool_call() {
        let text = r#"<think>I should check the file.</think>
<tool_call>{"name":"run","args":{"command":"ls"}}</tool_call>"#;
        let cleaned = strip_thinking(text);
        let calls = parse_tool_calls(&cleaned);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "run");
    }

    #[test]
    fn strip_thinking_empty() {
        assert_eq!(strip_thinking("No thinking here."), "No thinking here.");
    }
}
