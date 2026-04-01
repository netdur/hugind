use crate::core::orchestrator::memory::SharedMemory;
use crate::core::orchestrator::messaging::MessageBus;
use crate::core::orchestrator::task::TaskQueue;
use parking_lot::Mutex;
use std::sync::Arc;

/// Shared team context passed into agent runtimes.
/// Allows agents to read/write shared memory, send messages,
/// and spawn tasks during execution.
#[derive(Clone)]
pub struct TeamContext {
    pub agent_name: String,
    pub memory: SharedMemory,
    pub messages: MessageBus,
    pub task_queue: Option<Arc<Mutex<TaskQueue>>>,
}

impl TeamContext {
    pub fn new(agent_name: &str, memory: SharedMemory, messages: MessageBus) -> Self {
        Self {
            agent_name: agent_name.to_string(),
            memory,
            messages,
            task_queue: None,
        }
    }

    pub fn with_task_queue(mut self, queue: Arc<Mutex<TaskQueue>>) -> Self {
        self.task_queue = Some(queue);
        self
    }
}
