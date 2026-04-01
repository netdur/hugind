use crate::core::orchestrator::memory::SharedMemory;
use crate::core::orchestrator::task::Task;
use anyhow::Result;
use serde::Deserialize;

/// A task definition as returned by the coordinator LLM.
#[derive(Debug, Deserialize)]
struct CoordinatorTask {
    title: String,
    description: String,
    assignee: String,
    #[serde(default, rename = "dependsOn")]
    depends_on: Vec<String>,
}

/// Build the system prompt for the coordinator agent.
pub fn build_coordinator_prompt(agents: &[(String, String)]) -> String {
    let mut prompt = String::from(
        "You are a project coordinator. Your team consists of:\n\n",
    );

    for (name, description) in agents {
        if description.is_empty() {
            prompt.push_str(&format!("- {}\n", name));
        } else {
            prompt.push_str(&format!("- {}: {}\n", name, description));
        }
    }

    // Include available skills so the coordinator can mention them in task descriptions
    let skills = crate::core::skill::load_all_skills().unwrap_or_default();
    if !skills.is_empty() {
        prompt.push_str("\nAvailable skills that agents can activate:\n");
        for skill in &skills {
            prompt.push_str(&format!("- {}: {}\n", skill.config.name, skill.config.description));
        }
        prompt.push_str("\nYou may mention relevant skills in task descriptions to guide agents.\n");
    }

    prompt.push_str(
        "\nDecompose the following goal into a set of tasks. Each task should have:\n\
         - title (short, unique)\n\
         - description (what to do)\n\
         - assignee (one of the team member names listed above)\n\
         - dependsOn (array of task titles this task depends on, or empty array)\n\n\
         Respond with a JSON array of tasks. No other text.\n",
    );

    prompt
}

/// Build the synthesis prompt after all tasks are complete.
pub fn build_synthesis_prompt(memory: &SharedMemory) -> String {
    let summary = memory.summary();
    format!(
        "All tasks are complete. Here is the shared memory with all agent outputs:\n\n\
         {}\n\n\
         Synthesize a final summary of what was accomplished. Be concise.",
        summary
    )
}

/// Parse the coordinator's response into a list of tasks.
pub fn parse_coordinator_tasks(response: &str) -> Result<Vec<Task>> {
    // Try to find JSON array in the response (may be wrapped in markdown)
    let json_str = extract_json_array(response)
        .ok_or_else(|| anyhow::anyhow!("Coordinator did not return a JSON array of tasks"))?;

    let raw_tasks: Vec<CoordinatorTask> = serde_json::from_str(&json_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse coordinator tasks: {}", e))?;

    let tasks: Vec<Task> = raw_tasks
        .into_iter()
        .enumerate()
        .map(|(i, ct)| {
            let mut task = Task::new(
                &format!("task-{}", i),
                &ct.title,
                &ct.description,
            );
            task.assignee = Some(ct.assignee);
            task.depends_on = ct.depends_on;
            task
        })
        .collect();

    Ok(tasks)
}

/// Extract a JSON array from text that may contain markdown fences or other wrapping.
fn extract_json_array(text: &str) -> Option<String> {
    // Try direct parse first
    if text.trim().starts_with('[') {
        if serde_json::from_str::<serde_json::Value>(text.trim()).is_ok() {
            return Some(text.trim().to_string());
        }
    }

    // Try extracting from markdown code fence
    if let Some(start) = text.find("```json") {
        let after = &text[start + 7..];
        if let Some(end) = after.find("```") {
            let json_str = after[..end].trim();
            if serde_json::from_str::<serde_json::Value>(json_str).is_ok() {
                return Some(json_str.to_string());
            }
        }
    }

    // Try finding first [ ... ] pair
    if let Some(start) = text.find('[') {
        if let Some(end) = text.rfind(']') {
            if end > start {
                let candidate = &text[start..=end];
                if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
                    return Some(candidate.to_string());
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_coordinator_output() {
        let response = r#"[
            {"title": "Design", "description": "Design the API", "assignee": "architect", "dependsOn": []},
            {"title": "Implement", "description": "Build it", "assignee": "developer", "dependsOn": ["Design"]}
        ]"#;

        let tasks = parse_coordinator_tasks(response).unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].title, "Design");
        assert_eq!(tasks[0].assignee.as_deref(), Some("architect"));
        assert_eq!(tasks[1].depends_on, vec!["Design"]);
    }

    #[test]
    fn parse_markdown_wrapped() {
        let response = "Here are the tasks:\n```json\n[{\"title\": \"X\", \"description\": \"do X\", \"assignee\": \"a\", \"dependsOn\": []}]\n```\n";
        let tasks = parse_coordinator_tasks(response).unwrap();
        assert_eq!(tasks.len(), 1);
    }

    #[test]
    fn build_prompt_includes_agents() {
        let agents = vec![
            ("architect".to_string(), "designs systems".to_string()),
            ("developer".to_string(), "writes code".to_string()),
        ];
        let prompt = build_coordinator_prompt(&agents);
        assert!(prompt.contains("architect: designs systems"));
        assert!(prompt.contains("developer: writes code"));
    }
}
