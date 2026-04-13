use crate::engine::EngineStats;
use crate::engine::request::{Request, ThinkingMarkers};
use crate::llm::sampling::SamplingConfig;
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct AppState {
    pub engine_tx: mpsc::Sender<Request>,
    pub kv_manager: Arc<crate::engine::kv_cache::KvCacheManager>,
    pub engine_stats: Arc<RwLock<EngineStats>>,
    pub model: Arc<crate::llm::model::Model>,
    pub model_name: Option<String>,
    pub config_name: Option<String>,
    pub api_key: Option<String>,
    pub embeddings_enabled: bool,
    pub enable_thinking_default: bool,
    pub thinking_budget_tokens_default: Option<u32>,
    pub sampling_defaults: SamplingConfig,
    pub system_prompt: Option<String>,
    pub thinking_markers: ThinkingMarkers,
}
