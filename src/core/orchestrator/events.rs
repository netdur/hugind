use serde::Serialize;
use std::time::Instant;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    WorkflowStart { name: String },
    TaskReady { task_id: String, title: String, assignee: Option<String> },
    TaskStart { task_id: String, title: String, assignee: String },
    TaskComplete { task_id: String, title: String },
    TaskFailed { task_id: String, title: String, error: String },
    AgentMessage { from: String, to: String },
    MemoryWrite { agent: String, key: String },
    WorkflowComplete { success: bool },
}

#[derive(Debug, Clone)]
pub struct OrchestratorEvent {
    pub timestamp: Instant,
    pub kind: EventKind,
}

impl OrchestratorEvent {
    pub fn new(kind: EventKind) -> Self {
        Self {
            timestamp: Instant::now(),
            kind,
        }
    }
}

/// Event emitter for orchestration progress.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<OrchestratorEvent>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { tx }
    }

    pub fn emit(&self, kind: EventKind) {
        let _ = self.tx.send(OrchestratorEvent::new(kind));
    }

    pub fn subscribe(&self) -> broadcast::Receiver<OrchestratorEvent> {
        self.tx.subscribe()
    }
}
