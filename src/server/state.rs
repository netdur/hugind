use std::sync::Arc;
use tokio::sync::mpsc;
use crate::engine::request::Request;
use crate::engine::EngineStats;
use parking_lot::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub engine_tx: mpsc::Sender<Request>,
    pub kv_manager: Arc<crate::engine::kv_cache::KvCacheManager>, 
    pub engine_stats: Arc<RwLock<EngineStats>>, 
    pub model: Arc<crate::llm::model::Model>,
    pub model_name: Option<String>,
    pub config_name: Option<String>,
    pub api_key: Option<String>,
}
