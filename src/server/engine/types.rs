use crate::llm::error::Error;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};


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
    
    Text { text: String, request: RequestHandle },
    
    Finish { request: RequestHandle, reason: StopReason },
    
    Embedding { embedding: Vec<f32>, request: RequestHandle },
    
    Error { message: String, request: RequestHandle },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    Eos,
    MaxTokens,
    StopString,
    Cancelled,
}


pub fn err(msg: impl Into<String>) -> Error {
    Error::BackendError(msg.into())
}
