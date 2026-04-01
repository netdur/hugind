use parking_lot::RwLock;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::Arc;

/// Namespaced key-value store for cross-agent knowledge sharing.
///
/// Keys are stored as `agent_name/key`. Agents write under their own namespace
/// and can read any namespace.
#[derive(Clone)]
pub struct SharedMemory {
    store: Arc<RwLock<HashMap<String, JsonValue>>>,
}

impl SharedMemory {
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Write a value under `agent_name/key`.
    pub fn set(&self, agent: &str, key: &str, value: JsonValue) {
        let full_key = format!("{}/{}", agent, key);
        self.store.write().insert(full_key, value);
    }

    /// Read a value by fully-qualified key (e.g. "researcher/findings").
    pub fn get(&self, full_key: &str) -> Option<JsonValue> {
        self.store.read().get(full_key).cloned()
    }

    /// List all entries written by a specific agent.
    pub fn list_by_agent(&self, agent: &str) -> Vec<(String, JsonValue)> {
        let prefix = format!("{}/", agent);
        self.store
            .read()
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// List all entries.
    pub fn list_all(&self) -> Vec<(String, JsonValue)> {
        self.store
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Produce a markdown summary grouped by agent, suitable for prompt injection.
    pub fn summary(&self) -> String {
        let store = self.store.read();
        if store.is_empty() {
            return String::new();
        }

        let mut by_agent: HashMap<&str, Vec<(&str, &JsonValue)>> = HashMap::new();
        for (key, value) in store.iter() {
            if let Some((agent, subkey)) = key.split_once('/') {
                by_agent.entry(agent).or_default().push((subkey, value));
            }
        }

        let mut agents: Vec<&str> = by_agent.keys().copied().collect();
        agents.sort();

        let mut out = String::from("## Shared Team Memory\n\n");
        for agent in agents {
            out.push_str(&format!("### {}\n", agent));
            let entries = &by_agent[agent];
            for (key, value) in entries {
                let preview = match value {
                    JsonValue::String(s) => {
                        if s.len() > 200 {
                            format!("{}...", &s[..200])
                        } else {
                            s.clone()
                        }
                    }
                    other => {
                        let s = serde_json::to_string(other).unwrap_or_default();
                        if s.len() > 200 {
                            format!("{}...", &s[..200])
                        } else {
                            s
                        }
                    }
                };
                out.push_str(&format!("- {}: {}\n", key, preview));
            }
            out.push('\n');
        }
        out
    }

    /// Convert to JSON object for injection into agent initial data.
    pub fn to_json(&self) -> JsonValue {
        let store = self.store.read();
        let map: serde_json::Map<String, JsonValue> = store
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        JsonValue::Object(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn set_and_get() {
        let mem = SharedMemory::new();
        mem.set("agent1", "result", json!("hello"));
        assert_eq!(mem.get("agent1/result"), Some(json!("hello")));
        assert_eq!(mem.get("agent1/missing"), None);
    }

    #[test]
    fn list_by_agent_filters() {
        let mem = SharedMemory::new();
        mem.set("a", "x", json!(1));
        mem.set("a", "y", json!(2));
        mem.set("b", "z", json!(3));
        let a_entries = mem.list_by_agent("a");
        assert_eq!(a_entries.len(), 2);
        let b_entries = mem.list_by_agent("b");
        assert_eq!(b_entries.len(), 1);
    }

    #[test]
    fn summary_groups_by_agent() {
        let mem = SharedMemory::new();
        mem.set("researcher", "findings", json!("found X"));
        mem.set("coder", "plan", json!("implement Y"));
        let summary = mem.summary();
        assert!(summary.contains("### researcher"));
        assert!(summary.contains("### coder"));
        assert!(summary.contains("findings: found X"));
    }

    #[test]
    fn concurrent_access() {
        let mem = SharedMemory::new();
        let mem2 = mem.clone();
        let handle = std::thread::spawn(move || {
            mem2.set("thread", "value", json!(42));
        });
        handle.join().unwrap();
        assert_eq!(mem.get("thread/value"), Some(json!(42)));
    }
}
