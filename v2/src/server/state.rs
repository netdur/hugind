use std::sync::Arc;
use tokio::sync::mpsc;
use crate::engine::request::Request;
use crate::engine::EngineStats;
use parking_lot::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub engine_tx: mpsc::Sender<Request>,
    pub kv_manager: Arc<crate::engine::kv_cache::KvCacheManager>, // Shared for monitoring
    pub engine_stats: Arc<RwLock<EngineStats>>, // Shared for monitoring
    pub model: Arc<crate::llm::model::Model>,
    pub api_key: Option<String>,
}
