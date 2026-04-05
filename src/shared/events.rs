use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;

/// Structured events emitted by the agentic loop.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type")]
pub enum AgentEvent {
    #[serde(rename = "agent.setup")]
    Setup { tool_count: usize },
    #[serde(rename = "agent.turn")]
    Turn {
        turn: usize,
        max_turns: usize,
        message_count: usize,
    },
    #[serde(rename = "agent.tool_call")]
    ToolCall {
        name: String,
        args: serde_json::Value,
    },
    #[serde(rename = "agent.tool_result")]
    ToolResult {
        name: String,
        result: String,
        duration_ms: u64,
    },
    #[serde(rename = "agent.progress")]
    Progress { message: String },
    #[serde(rename = "agent.complete")]
    Complete { turns: usize, final_len: usize },
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: AgentEvent);
}

fn sink_lock() -> &'static RwLock<Option<Arc<dyn EventSink>>> {
    static LOCK: OnceLock<RwLock<Option<Arc<dyn EventSink>>>> = OnceLock::new();
    LOCK.get_or_init(|| RwLock::new(None))
}

pub fn set_event_sink(sink: Option<Arc<dyn EventSink>>) {
    *sink_lock().write() = sink;
}

pub fn has_sink() -> bool {
    sink_lock().read().is_some()
}

pub fn emit(event: AgentEvent) {
    if let Some(sink) = sink_lock().read().as_ref() {
        sink.emit(event);
    }
}
