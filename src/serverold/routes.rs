use std::sync::Arc;

use axum::{
    routing::{get, post},
    Router,
};

use crate::server::{openai, state::AppState, monitor};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/monitor", get(monitor::monitor))
        .route("/v1/models", get(openai::list_models))
        .route("/v1/chat/completions", post(openai::chat_completions))
        .route("/v1/chat/*any", post(openai::chat_fallback))
        .route("/v1/completions", post(openai::completions))
        .route("/v1/embeddings", post(openai::embeddings))
        .route("/v1/chat/hibernate", post(openai::chat_hibernate))
        .route("/v1/chat/delete", post(openai::chat_delete))
        .with_state(state)
}

async fn health(axum::extract::State(state): axum::extract::State<Arc<AppState>>) -> axum::response::Json<serde_json::Value> {
    axum::response::Json(serde_json::json!({
        "status": "ok",
        "model": state.model_name
    }))
}
