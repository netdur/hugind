use crate::engine::request::{Request, ThinkingMarkers};
use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderMap, header::AUTHORIZATION},
    response::IntoResponse,
    routing::{get, post},
};
use std::sync::Arc;
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

pub mod engine;
pub mod llm;
pub mod routes;
pub mod state;
pub mod types;

fn build_app(state: Arc<state::AppState>) -> Router {
    const MAX_REQUEST_BODY_BYTES: usize = 20 * 1024 * 1024; // 20 MiB

    Router::new()
        .route("/v1/chat/completions", post(routes::chat_completions))
        .route("/v1/models", get(routes::list_models))
        .route("/v1/monitor", get(routes::monitor))
        .route("/v1/state/save", post(routes::save_state))
        .route("/v1/state/idle", post(routes::idle_state))
        .route(
            "/v1/state/:id",
            get(routes::get_state).delete(routes::delete_state),
        )
        .route("/v1/embeddings", post(routes::embeddings))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(state)
}

pub async fn run_server(
    engine_tx: mpsc::Sender<Request>,
    kv_manager: Arc<crate::engine::kv_cache::KvCacheManager>,
    engine_stats: Arc<parking_lot::RwLock<crate::engine::EngineStats>>,
    model: Arc<crate::llm::model::Model>,
    model_name: Option<String>,
    config_name: Option<String>,
    embeddings_enabled: bool,
    enable_thinking_default: bool,
    thinking_budget_tokens_default: Option<u32>,
    sampling_defaults: crate::llm::sampling::SamplingConfig,
    system_prompt: Option<String>,
    host: String,
    port: u16,
    api_key: Option<String>,
    thinking_markers: ThinkingMarkers,
) {
    let state = Arc::new(state::AppState {
        engine_tx,
        kv_manager,
        engine_stats,
        model,
        model_name,
        config_name,
        api_key,
        embeddings_enabled,
        enable_thinking_default,
        thinking_budget_tokens_default,
        sampling_defaults,
        system_prompt,
        thinking_markers,
    });

    let app = build_app(state);

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
        if let Err(message) = validate_bearer_auth(req.headers(), key) {
            return (axum::http::StatusCode::UNAUTHORIZED, message).into_response();
        }
    }
    next.run(req).await
}

fn validate_bearer_auth(headers: &HeaderMap, expected_key: &str) -> Result<(), &'static str> {
    let auth_header = headers
        .get(AUTHORIZATION)
        .ok_or("Missing Authorization Header")?;
    let auth_str = auth_header
        .to_str()
        .map_err(|_| "Invalid Authorization Header Encoding")?;
    if !auth_str.starts_with("Bearer ") {
        return Err("Invalid Authorization Header Format. Expected 'Bearer <key>'");
    }
    let token = auth_str[7..].as_bytes();
    let expected = expected_key.as_bytes();
    // Constant-time comparison to prevent timing attacks
    if token.len() != expected.len()
        || token
            .iter()
            .zip(expected.iter())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            != 0
    {
        return Err("Invalid API Key");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{build_app, validate_bearer_auth};
    use crate::engine::EngineStats;
    use crate::engine::kv_cache::KvCacheManager;
    use crate::llm::model::Model;
    use crate::llm::sampling::SamplingConfig;
    use crate::engine::request::ThinkingMarkers;
    use crate::server::state::AppState;
    use axum::body::{Body, to_bytes};
    use axum::http::{HeaderMap, HeaderValue, Request, StatusCode, header::AUTHORIZATION};
    use parking_lot::RwLock;
    use std::sync::Arc;
    use tower::ServiceExt;

    fn make_state(api_key: Option<&str>) -> Arc<AppState> {
        let (engine_tx, _engine_rx) = tokio::sync::mpsc::channel(8);
        Arc::new(AppState {
            engine_tx,
            kv_manager: Arc::new(KvCacheManager::new(false)),
            engine_stats: Arc::new(RwLock::new(EngineStats::default())),
            model: Arc::new(Model::dummy()),
            model_name: Some("test-model".to_string()),
            config_name: Some("test-config".to_string()),
            api_key: api_key.map(|v| v.to_string()),
            embeddings_enabled: false,
            enable_thinking_default: false,
            thinking_budget_tokens_default: None,
            sampling_defaults: SamplingConfig::default(),
            system_prompt: None,
            thinking_markers: ThinkingMarkers::default(),
        })
    }

    async fn body_text(resp: axum::response::Response) -> String {
        let bytes = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        String::from_utf8_lossy(&bytes).to_string()
    }

    #[test]
    fn validate_bearer_auth_accepts_valid_token() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer secret"));
        assert!(validate_bearer_auth(&headers, "secret").is_ok());
    }

    #[test]
    fn validate_bearer_auth_rejects_missing_header() {
        let headers = HeaderMap::new();
        let err = validate_bearer_auth(&headers, "secret").expect_err("should fail");
        assert_eq!(err, "Missing Authorization Header");
    }

    #[test]
    fn validate_bearer_auth_rejects_malformed_header() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Token secret"));
        let err = validate_bearer_auth(&headers, "secret").expect_err("should fail");
        assert_eq!(
            err,
            "Invalid Authorization Header Format. Expected 'Bearer <key>'"
        );
    }

    #[test]
    fn validate_bearer_auth_rejects_wrong_key() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer nope"));
        let err = validate_bearer_auth(&headers, "secret").expect_err("should fail");
        assert_eq!(err, "Invalid API Key");
    }

    #[test]
    fn validate_bearer_auth_rejects_non_utf8_encoding() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_bytes(b"Bearer \xFF").expect("header bytes"),
        );
        let err = validate_bearer_auth(&headers, "secret").expect_err("should fail");
        assert_eq!(err, "Invalid Authorization Header Encoding");
    }

    #[tokio::test]
    async fn router_allows_requests_when_api_key_is_not_configured() {
        let app = build_app(make_state(None));
        let req = Request::builder()
            .uri("/v1/models")
            .method("GET")
            .body(Body::empty())
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn router_rejects_missing_auth_header_when_api_key_is_configured() {
        let app = build_app(make_state(Some("secret")));
        let req = Request::builder()
            .uri("/v1/models")
            .method("GET")
            .body(Body::empty())
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = body_text(resp).await;
        assert!(body.contains("Missing Authorization Header"));
    }

    #[tokio::test]
    async fn router_rejects_invalid_bearer_token() {
        let app = build_app(make_state(Some("secret")));
        let req = Request::builder()
            .uri("/v1/models")
            .method("GET")
            .header(AUTHORIZATION, "Bearer wrong")
            .body(Body::empty())
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = body_text(resp).await;
        assert!(body.contains("Invalid API Key"));
    }

    #[tokio::test]
    async fn router_accepts_valid_bearer_token() {
        let app = build_app(make_state(Some("secret")));
        let req = Request::builder()
            .uri("/v1/models")
            .method("GET")
            .header(AUTHORIZATION, "Bearer secret")
            .body(Body::empty())
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn router_applies_auth_before_chat_handler() {
        let app = build_app(make_state(Some("secret")));
        let req = Request::builder()
            .uri("/v1/chat/completions")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"model":"test","messages":[{"role":"user","content":"hi"}]}"#,
            ))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn router_reaches_chat_handler_when_authorized() {
        let app = build_app(make_state(Some("secret")));
        let req = Request::builder()
            .uri("/v1/chat/completions")
            .method("POST")
            .header(AUTHORIZATION, "Bearer secret")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"model":"test","messages":[]}"#))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        // Auth should pass; handler then rejects empty messages.
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_text(resp).await;
        assert!(body.contains("Messages cannot be empty"));
    }
}
