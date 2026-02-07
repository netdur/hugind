use crate::llm::error::Error;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

/// High-level event emitted by the engine.
#[derive(Debug, Clone)]
pub struct Event {
    pub id: String,
    pub kind: EventKind,
}

#[derive(Debug, Clone)]
pub struct RequestHandle {
    id: String,
    cancel_flag: Arc<AtomicBool>,
}

impl RequestHandle {
    pub fn new(id: String, cancel_flag: Arc<AtomicBool>) -> Self {
        Self { id, cancel_flag }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone)]
pub enum EventKind {
    /// A piece of generated text (streaming).
    Text { text: String, request: RequestHandle },
    /// Request finished.
    Finish { request: RequestHandle, reason: StopReason },
    /// Generated Embedding
    Embedding { embedding: Vec<f32>, request: RequestHandle },
    /// Error during processing.
    Error { message: String, request: RequestHandle },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    Eos,
    MaxTokens,
    StopString,
    Cancelled,
}

/// Helper to wrap simple errors
pub fn err(msg: impl Into<String>) -> Error {
    Error::BackendError(msg.into())
}
