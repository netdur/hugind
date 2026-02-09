use axum::{
    routing::{get, post},
    Router,
    response::IntoResponse,
};
use tower_http::trace::TraceLayer;
use tower_http::cors::CorsLayer;
use std::sync::Arc;
use tokio::sync::mpsc;
use crate::engine::request::Request;

pub mod state;
pub mod types;
pub mod routes;
pub mod engine;
pub mod llm;

pub async fn run_server(
    engine_tx: mpsc::Sender<Request>,
    kv_manager: Arc<crate::engine::kv_cache::KvCacheManager>,
    engine_stats: Arc<parking_lot::RwLock<crate::engine::EngineStats>>,
    model: Arc<crate::llm::model::Model>,
    model_name: Option<String>,
    config_name: Option<String>,
    host: String,
    port: u16,
    api_key: Option<String>,
) {
    

    let state = Arc::new(state::AppState {
        engine_tx,
        kv_manager,
        engine_stats,
        model,
        model_name,
        config_name,
        api_key,
    });

    let app = Router::new()
        
        .route("/v1/chat/completions", post(routes::chat_completions))
        .route("/v1/models", get(routes::list_models))
        
        .route("/v1/monitor", get(routes::monitor))
        .route("/v1/state/save", post(routes::save_state))
        .route("/v1/state/idle", post(routes::idle_state))
        .route("/v1/state/:id", axum::routing::delete(routes::delete_state))
        .route("/v1/embeddings", post(routes::embeddings))
        
        
        .layer(axum::middleware::from_fn_with_state(state.clone(), auth_middleware))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("{}:{}", host, port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("Failed to bind to {}: {}", addr, e);
            eprintln!("Is another instance already running on this port?");
            return;
        }
    };

    match listener.local_addr() {
        Ok(local_addr) => println!("Server listening on {}", local_addr),
        Err(e) => eprintln!("Server bound, but failed to read local addr: {}", e),
    }

    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("Server error: {}", e);
    }
}

async fn auth_middleware(
    axum::extract::State(state): axum::extract::State<Arc<state::AppState>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if let Some(ref key) = state.api_key {
        let auth_header = req.headers().get(axum::http::header::AUTHORIZATION);
        match auth_header {
            Some(header_value) => {
                if let Ok(header_str) = header_value.to_str() {
                    if header_str.starts_with("Bearer ") {
                        let token = &header_str[7..];
                        if token != key {
                             return (axum::http::StatusCode::UNAUTHORIZED, "Invalid API Key").into_response();
                        }
                    } else {
                         return (axum::http::StatusCode::UNAUTHORIZED, "Invalid Authorization Header Format. Expected 'Bearer <key>'").into_response();
                    }
                } else {
                     return (axum::http::StatusCode::UNAUTHORIZED, "Invalid Authorization Header Encoding").into_response();
                }
            }
            None => {
                 return (axum::http::StatusCode::UNAUTHORIZED, "Missing Authorization Header").into_response();
            }
        }
    }
    next.run(req).await
}
