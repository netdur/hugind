use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{sse::{Event, KeepAlive, Sse}, IntoResponse, Response},
    Json,
};
use serde_json::json;
use serde_json::Value as JsonValue;
use std::convert::Infallible;
use chrono::Utc;
use uuid::Uuid;
use tokio_stream::wrappers::UnboundedReceiverStream;
use std::sync::atomic::Ordering;
use futures_util::stream::StreamExt;

use crate::server::state::AppState;
use crate::server::llm_backend::service::StreamEvent;

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": {
                "message": "Unauthorized",
                "type": "auth_error"
            }
        })),
    )
        .into_response()
}

fn not_implemented() -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": {
                "message": "Not implemented",
                "type": "not_implemented"
            }
        })),
    )
        .into_response()
}

fn bad_request(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": {
                "message": message,
                "type": "invalid_request_error"
            }
        })),
    )
        .into_response()
}

fn server_error(message: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": {
                "message": message,
                "type": "server_error"
            }
        })),
    )
        .into_response()
}

struct RequestGuard {
    state: Arc<AppState>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl RequestGuard {
    fn new(state: Arc<AppState>, permit: tokio::sync::OwnedSemaphorePermit) -> Self {
        state.active.fetch_add(1, Ordering::Relaxed);
        Self { state, _permit: permit }
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        self.state.active.fetch_sub(1, Ordering::Relaxed);
    }
}

fn next_session_id(prefix: &str) -> String {
    format!(
        "{}-{}-{}",
        prefix,
        Utc::now().timestamp_millis(),
        Uuid::new_v4().as_u128() % 1000
    )
}

fn extract_message_text(value: &JsonValue) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    let items = value.as_array()?;
    let mut combined = String::new();
    for item in items {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if item_type == "text" {
            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(text);
            }
        }
    }
    if combined.is_empty() {
        None
    } else {
        Some(combined)
    }
}

pub async fn list_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if !state.is_authorized(&headers) {
        return unauthorized();
    }

    Json(json!({
        "object": "list",
        "data": [
            {
                "id": state.model_name,
                "object": "model"
            }
        ]
    }))
    .into_response()
}

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<JsonValue>,
) -> Response {
    if !state.is_authorized(&headers) {
        return unauthorized();
    }
    let stream = payload.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    let created = Utc::now().timestamp();
    let model = payload
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or(&state.model_name)
        .to_string();

    let messages = match payload.get("messages").and_then(|v| v.as_array()) {
        Some(m) if !m.is_empty() => m,
        _ => return bad_request("Missing or empty messages array."),
    };

    let session_id = next_session_id("chatcmpl");
    state.waiting.fetch_add(1, Ordering::Relaxed);
    let permit = match state.semaphore.clone().acquire_owned().await {
        Ok(permit) => permit,
        Err(_) => {
            state.waiting.fetch_sub(1, Ordering::Relaxed);
            return server_error("Server is shutting down.");
        }
    };
    state.waiting.fetch_sub(1, Ordering::Relaxed);
    let guard = RequestGuard::new(Arc::clone(&state), permit);
    let service = state.manager.service();
    if let Err(e) = service.load_session(&session_id) {
        drop(guard);
        return server_error(&format!("Failed to load session: {}", e));
    }

    let sampler_params = state.manager.sampler_params().clone();
    let system_prompt = state.manager.config().system_prompt.trim().to_string();
    let has_system = messages.iter().any(|m| {
        m.get("role")
            .and_then(|v| v.as_str())
            .map(|r| r == "system")
            .unwrap_or(false)
    });

    if !system_prompt.is_empty() && !has_system {
        if let Err(e) = service.add_message(&session_id, "system", &system_prompt, Vec::new(), &sampler_params) {
            drop(guard);
            return server_error(&format!("Failed to add system prompt: {}", e));
        }
    }

    for message in messages {
        let role = match message.get("role").and_then(|v| v.as_str()) {
            Some(r) => r,
            None => return bad_request("Message role is missing."),
        };
        let content = match message.get("content").and_then(extract_message_text) {
            Some(c) => c,
            None => return bad_request("Message content is missing or unsupported."),
        };
        if let Err(e) = service.add_message(&session_id, role, &content, Vec::new(), &sampler_params) {
            drop(guard);
            return server_error(&format!("Failed to add message: {}", e));
        }
    }

    if stream {
        let rx = service.register_stream(&session_id);
        let mut guard = Some(guard);
        let session_id = session_id.clone();
        let state = Arc::clone(&state);
        let model = model.clone();

        let stream = UnboundedReceiverStream::new(rx).flat_map(move |event| {
            let events: Vec<Result<Event, Infallible>> = match event {
                StreamEvent::Token(part) => {
                    let chunk = json!({
                        "id": session_id,
                        "object": "chat.completion.chunk",
                        "created": created,
                        "model": model,
                        "choices": [
                            {
                                "index": 0,
                                "delta": { "content": part },
                                "finish_reason": null
                            }
                        ]
                    });
                    vec![Ok(Event::default().data(chunk.to_string()))]
                }
                StreamEvent::Done => {
                    if let Some(guard) = guard.take() {
                        drop(guard);
                    }
                    let queue = state.queue_snapshot();
                    println!(
                        "[stream] completed {} | waiting={} active={} available={} max={}",
                        session_id, queue.waiting, queue.active, queue.available, queue.max_slots
                    );
                    let final_chunk = json!({
                        "id": session_id,
                        "object": "chat.completion.chunk",
                        "created": created,
                        "model": model,
                        "choices": [
                            {
                                "index": 0,
                                "delta": {},
                                "finish_reason": "stop"
                            }
                        ]
                    });
                    vec![
                        Ok(Event::default().data(final_chunk.to_string())),
                        Ok(Event::default().data("[DONE]")),
                    ]
                }
            };
            futures_util::stream::iter(events)
        });

        return Sse::new(stream)
            .keep_alive(KeepAlive::default().interval(Duration::from_secs(15)))
            .into_response();
    }

    let mut rx = service.register_stream(&session_id);
    let mut output = String::new();
    while let Some(event) = rx.recv().await {
        match event {
            StreamEvent::Token(part) => output.push_str(&part),
            StreamEvent::Done => break,
        }
    }
    drop(guard);
    let queue = state.queue_snapshot();
    println!(
        "[sync] completed {} | waiting={} active={} available={} max={}",
        session_id, queue.waiting, queue.active, queue.available, queue.max_slots
    );

    Json(json!({
        "id": session_id,
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [
            {
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": output
                },
                "finish_reason": "stop"
            }
        ]
    }))
    .into_response()
}

pub async fn completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<JsonValue>,
) -> Response {
    if !state.is_authorized(&headers) {
        return unauthorized();
    }
    let stream = payload.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    let created = Utc::now().timestamp();
    let model = payload
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or(&state.model_name)
        .to_string();

    let prompt = match payload.get("prompt").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => return bad_request("Missing prompt."),
    };

    let session_id = next_session_id("cmpl");
    state.waiting.fetch_add(1, Ordering::Relaxed);
    let permit = match state.semaphore.clone().acquire_owned().await {
        Ok(permit) => permit,
        Err(_) => {
            state.waiting.fetch_sub(1, Ordering::Relaxed);
            return server_error("Server is shutting down.");
        }
    };
    state.waiting.fetch_sub(1, Ordering::Relaxed);
    let guard = RequestGuard::new(Arc::clone(&state), permit);
    let service = state.manager.service();
    let sampler_params = state.manager.sampler_params().clone();
    if let Err(e) = service.load_session(&session_id) {
        drop(guard);
        return server_error(&format!("Failed to load session: {}", e));
    }
    if let Err(e) = service.add_request(&session_id, &prompt, &sampler_params) {
        drop(guard);
        return server_error(&format!("Failed to add prompt: {}", e));
    }

    if stream {
        let rx = service.register_stream(&session_id);
        let mut guard = Some(guard);
        let session_id = session_id.clone();
        let state = Arc::clone(&state);
        let model = model.clone();

        let stream = UnboundedReceiverStream::new(rx).flat_map(move |event| {
            let events: Vec<Result<Event, Infallible>> = match event {
                StreamEvent::Token(part) => {
                    let chunk = json!({
                        "id": session_id,
                        "object": "text_completion",
                        "created": created,
                        "model": model,
                        "choices": [
                            {
                                "index": 0,
                                "text": part,
                                "finish_reason": null
                            }
                        ]
                    });
                    vec![Ok(Event::default().data(chunk.to_string()))]
                }
                StreamEvent::Done => {
                    if let Some(guard) = guard.take() {
                        drop(guard);
                    }
                    let queue = state.queue_snapshot();
                    println!(
                        "[stream] completed {} | waiting={} active={} available={} max={}",
                        session_id, queue.waiting, queue.active, queue.available, queue.max_slots
                    );
                    let final_chunk = json!({
                        "id": session_id,
                        "object": "text_completion",
                        "created": created,
                        "model": model,
                        "choices": [
                            {
                                "index": 0,
                                "text": "",
                                "finish_reason": "stop"
                            }
                        ]
                    });
                    vec![
                        Ok(Event::default().data(final_chunk.to_string())),
                        Ok(Event::default().data("[DONE]")),
                    ]
                }
            };
            futures_util::stream::iter(events)
        });

        return Sse::new(stream)
            .keep_alive(KeepAlive::default().interval(Duration::from_secs(15)))
            .into_response();
    }

    let mut rx = service.register_stream(&session_id);
    let mut output = String::new();
    while let Some(event) = rx.recv().await {
        match event {
            StreamEvent::Token(part) => output.push_str(&part),
            StreamEvent::Done => break,
        }
    }
    drop(guard);
    let queue = state.queue_snapshot();
    println!(
        "[sync] completed {} | waiting={} active={} available={} max={}",
        session_id, queue.waiting, queue.active, queue.available, queue.max_slots
    );

    Json(json!({
        "id": session_id,
        "object": "text_completion",
        "created": created,
        "model": model,
        "choices": [
            {
                "index": 0,
                "text": output,
                "finish_reason": "stop"
            }
        ]
    }))
    .into_response()
}

pub async fn embeddings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if !state.is_authorized(&headers) {
        return unauthorized();
    }
    not_implemented()
}

pub async fn chat_hibernate(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if !state.is_authorized(&headers) {
        return unauthorized();
    }
    Json(json!({
        "status": "ok",
        "message": "hello world"
    }))
    .into_response()
}

pub async fn chat_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if !state.is_authorized(&headers) {
        return unauthorized();
    }
    Json(json!({
        "status": "ok",
        "message": "hello world"
    }))
    .into_response()
}

pub async fn chat_fallback(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if !state.is_authorized(&headers) {
        return unauthorized();
    }
    Json(json!({
        "status": "ok",
        "message": "hello world"
    }))
    .into_response()
}
