use parking_lot::RwLock;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct Message {
    pub id: usize,
    pub from: String,
    pub to: String,
    pub content: String,
    pub timestamp: Instant,
}

/// In-memory pub/sub message bus for inter-agent communication.
#[derive(Clone)]
pub struct MessageBus {
    messages: Arc<RwLock<Vec<Message>>>,
    /// Tracks which message IDs each agent has already read.
    read_by: Arc<RwLock<std::collections::HashMap<String, HashSet<usize>>>>,
    next_id: Arc<RwLock<usize>>,
}

impl MessageBus {
    pub fn new() -> Self {
        Self {
            messages: Arc::new(RwLock::new(Vec::new())),
            read_by: Arc::new(RwLock::new(std::collections::HashMap::new())),
            next_id: Arc::new(RwLock::new(0)),
        }
    }

    fn alloc_id(&self) -> usize {
        let mut id = self.next_id.write();
        let current = *id;
        *id += 1;
        current
    }

    /// Send a point-to-point message.
    pub fn send(&self, from: &str, to: &str, content: &str) {
        self.messages.write().push(Message {
            id: self.alloc_id(),
            from: from.to_string(),
            to: to.to_string(),
            content: content.to_string(),
            timestamp: Instant::now(),
        });
    }

    /// Broadcast a message to all agents (to = "*").
    pub fn broadcast(&self, from: &str, content: &str) {
        self.messages.write().push(Message {
            id: self.alloc_id(),
            from: from.to_string(),
            to: "*".to_string(),
            content: content.to_string(),
            timestamp: Instant::now(),
        });
    }

    /// Get all unread messages for an agent and mark them as read.
    pub fn receive(&self, agent: &str) -> Vec<Message> {
        let msgs = self.messages.read();
        let mut read_by = self.read_by.write();
        let read_set = read_by.entry(agent.to_string()).or_default();

        let mut result = Vec::new();
        for msg in msgs.iter() {
            if (msg.to == agent || msg.to == "*") && msg.from != agent && !read_set.contains(&msg.id) {
                read_set.insert(msg.id);
                result.push(msg.clone());
            }
        }
        result
    }

    /// Get all unread messages for an agent without marking them as read.
    pub fn peek_unread(&self, agent: &str) -> Vec<Message> {
        let msgs = self.messages.read();
        let read_by = self.read_by.read();
        let read_set = read_by.get(agent);

        msgs.iter()
            .filter(|m| {
                (m.to == agent || m.to == "*")
                    && m.from != agent
                    && read_set.map(|s| !s.contains(&m.id)).unwrap_or(true)
            })
            .cloned()
            .collect()
    }

    /// Format pending messages for prompt injection.
    pub fn format_for_prompt(&self, agent: &str) -> String {
        let messages = self.receive(agent);
        if messages.is_empty() {
            return String::new();
        }

        let mut out = String::from("## Messages from team members\n\n");
        for msg in &messages {
            let target = if msg.to == "*" { " (broadcast)" } else { "" };
            out.push_str(&format!("- **{}**{}: {}\n", msg.from, target, msg.content));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_to_point() {
        let bus = MessageBus::new();
        bus.send("alice", "bob", "hello");
        let msgs = bus.receive("bob");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "hello");

        // Alice should not receive her own message
        let msgs = bus.receive("alice");
        assert_eq!(msgs.len(), 0);
    }

    #[test]
    fn broadcast_reaches_all_except_sender() {
        let bus = MessageBus::new();
        bus.broadcast("coordinator", "stand by");

        let bob = bus.receive("bob");
        assert_eq!(bob.len(), 1);
        let alice = bus.receive("alice");
        assert_eq!(alice.len(), 1);
        let coord = bus.receive("coordinator");
        assert_eq!(coord.len(), 0);
    }

    #[test]
    fn receive_marks_as_read() {
        let bus = MessageBus::new();
        bus.send("a", "b", "first");
        let msgs = bus.receive("b");
        assert_eq!(msgs.len(), 1);

        // Second receive gets nothing
        let msgs = bus.receive("b");
        assert_eq!(msgs.len(), 0);
    }

    #[test]
    fn format_for_prompt() {
        let bus = MessageBus::new();
        bus.send("architect", "developer", "Spec is ready");
        bus.broadcast("coordinator", "Priority change");

        let prompt = bus.format_for_prompt("developer");
        assert!(prompt.contains("**architect**"));
        assert!(prompt.contains("Spec is ready"));
        assert!(prompt.contains("(broadcast)"));
    }
}
