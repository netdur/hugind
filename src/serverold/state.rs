use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Instant;
use tokio::sync::Semaphore;

use crate::server::manager::ServerManager;

pub struct AppState {
    pub model_name: String,
    pub api_key: Option<String>,
    pub started_at: Instant,
    pub manager: Arc<ServerManager>,
    pub semaphore: Arc<Semaphore>,
    pub max_slots: usize,
    pub waiting: AtomicUsize,
    pub active: AtomicUsize,
}

pub struct QueueSnapshot {
    pub waiting: usize,
    pub active: usize,
    pub available: usize,
    pub max_slots: usize,
}

impl AppState {
    pub fn queue_snapshot(&self) -> QueueSnapshot {
        QueueSnapshot {
            waiting: self.waiting.load(std::sync::atomic::Ordering::Relaxed),
            active: self.active.load(std::sync::atomic::Ordering::Relaxed),
            available: self.semaphore.available_permits(),
            max_slots: self.max_slots,
        }
    }
}

impl AppState {
    pub fn is_authorized(&self, headers: &HeaderMap) -> bool {
        let Some(expected) = self.api_key.as_deref() else {
            return true;
        };

        let Some(value) = headers.get(AUTHORIZATION) else {
            return false;
        };

        let Ok(value) = value.to_str() else {
            return false;
        };

        let value = value.trim();
        if let Some(token) = value.strip_prefix("Bearer ") {
            token == expected
        } else {
            false
        }
    }
}
