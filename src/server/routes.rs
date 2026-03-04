use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse,
        sse::{Event as SseEvent, Sse},
    },
};
use std::sync::Arc;
use tokio::sync::mpsc;

use std::time::{SystemTime, UNIX_EPOCH};

use crate::engine::request::{Request, RequestParams};
use crate::engine::types::EventKind;
use crate::llm::chat::{Message, apply_template};
use crate::llm::sampling::GrammarParams;
use crate::server::state::AppState;
use crate::server::types::*;
use crate::shared::paths;
use base64::Engine;
use std::time::Duration;

const JSON_GRAMMAR: &str = r#"
root   ::= object
value  ::= object | array | string | number | ("true" | "false" | "null") ws

object ::=
  "{" ws (
            string ":" ws value
    ("," ws string ":" ws value)*
  )? "}" ws

array  ::=
  "[" ws (
            value
    ("," ws value)*
  )? "]" ws

string ::=
  "\"" (
    [^"\\\x7F\x00-\x1F] |
    "\\" (["\\bfnrt] | "u" [0-9a-fA-F]{4}) # escapes
  )* "\"" ws

number ::= ("-"? ([0-9] | [1-9] [0-9]{0,15})) ("." [0-9]+)? ([eE] [-+]? [0-9] [1-9]{0,15})? ws

# Optional space: by convention, applied in this grammar after literal chars when allowed
ws ::= | " " | "\n" [ \t]{0,20}
"#;
const MTMD_MEDIA_MARKER: &str = "<__media__>";

fn extract_nonempty_header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|h| h.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn extract_session_id(headers: &HeaderMap) -> Option<String> {
    extract_nonempty_header(headers, "x-session-id")
        .or_else(|| extract_nonempty_header(headers, "x-request-id"))
}

fn parse_bool_header(headers: &HeaderMap, name: &str) -> bool {
    extract_nonempty_header(headers, name)
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn should_apply_system_prompt(
    state: &AppState,
    session_id: Option<&str>,
    fresh_session: bool,
) -> bool {
    if fresh_session {
        return true;
    }

    match session_id {
        None => true,
        Some(id) => {
            let sessions = state.kv_manager.sessions.read();
            !sessions.contains_key(id)
        }
    }
}

fn classify_chat_error(message: &str) -> (StatusCode, serde_json::Value) {
    if message.contains("Context shift unsupported by backend for this model") {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            serde_json::json!({
                "error": {
                    "message": "Context reached: context shifting is not supported by this model.",
                    "type": "invalid_request_error",
                    "param": null,
                    "code": "context_shift_unsupported"
                }
            }),
        );
    }

    (
        StatusCode::INTERNAL_SERVER_ERROR,
        serde_json::json!({
            "error": {
                "message": message,
                "type": "server_error",
                "param": null,
                "code": null
            }
        }),
    )
}

async fn load_image_bytes(url: &str) -> Result<Vec<u8>, String> {
    if url.starts_with("data:") {
        let (_, data_part) = url
            .split_once("base64,")
            .ok_or("Invalid data URL (missing base64)".to_string())?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data_part.as_bytes())
            .map_err(|e| format!("Failed to decode base64 image: {}", e))?;
        return Ok(bytes);
    }

    if url.starts_with("http://") || url.starts_with("https://") {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
        let resp = client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch image URL: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!("Image URL returned HTTP {}", resp.status()));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("Failed to read image bytes: {}", e))?;
        return Ok(bytes.to_vec());
    }

    Err("image_url must be a data URL or http(s) URL".to_string())
}

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ChatCompletionRequest>,
) -> impl IntoResponse {
    if payload.messages.is_empty() {
        return (StatusCode::BAD_REQUEST, "Messages cannot be empty").into_response();
    }

    let session_id = extract_session_id(&headers);
    let parent_id = extract_nonempty_header(&headers, "x-parent-id");
    let fresh_session = parse_bool_header(&headers, "x-fresh-session");

    let mut params = RequestParams::default();
    params.sampling = state.sampling_defaults.clone();

    let mut images = Vec::new();
    let mut messages = Vec::with_capacity(payload.messages.len());
    let mut has_system_message = false;

    for msg in &payload.messages {
        if msg.role.eq_ignore_ascii_case("system") {
            has_system_message = true;
        }

        let mut content = String::new();
        match &msg.content {
            Content::Text(t) => {
                content.push_str(t);
            }
            Content::Multimodal(parts) => {
                for part in parts {
                    match part {
                        MultimodalContent::Text { text } => content.push_str(text),
                        MultimodalContent::ImageUrl { image_url } => {
                            match load_image_bytes(&image_url.url).await {
                                Ok(data) => {
                                    images.push(data);
                                    content.push_str(MTMD_MEDIA_MARKER);
                                }
                                Err(e) => {
                                    return (StatusCode::BAD_REQUEST, e).into_response();
                                }
                            }
                        }
                    }
                }
            }
        }
        messages.push(Message::new(&msg.role, &content));
    }

    if !has_system_message {
        if let Some(system_prompt) = &state.system_prompt {
            if should_apply_system_prompt(state.as_ref(), session_id.as_deref(), fresh_session) {
                messages.insert(0, Message::new("system", system_prompt));
            }
        }
    }

    let full_prompt = match apply_template(
        &state.model,
        &messages,
        payload.enable_thinking,
        state.enable_thinking_default,
    ) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Prompt template error: {}", e),
            )
                .into_response();
        }
    };

    params.prompt = full_prompt;
    params.images = images;
    params.max_output_tokens = payload.max_tokens.map(|n| n as i32).unwrap_or(1024);
    params.sampling.temp = payload.temperature.unwrap_or(params.sampling.temp);
    params.sampling.top_p = payload.top_p.unwrap_or(params.sampling.top_p);

    if let Some(fp) = payload.frequency_penalty {
        params.sampling.penalty_repeat = 1.0 + fp.max(0.0);
    }

    if let Some(fmt) = &payload.response_format {
        if fmt.format_type == "json_object" {
            params.sampling.grammar = Some(GrammarParams {
                grammar: JSON_GRAMMAR.to_string(),
                root: "root".to_string(),
            });
        }
    }

    params.session_id = session_id;
    params.parent_id = parent_id;

    let (response_tx, mut response_rx) = mpsc::unbounded_channel();

    let mut req = Request::new(params);
    req.response_tx = Some(response_tx);

    if state.engine_tx.send(req).await.is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Engine buffer full or closed",
        )
            .into_response();
    }

    let is_stream = payload.stream.unwrap_or(false);
    let model_id = payload.model.clone();
    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    if is_stream {
        let stream = async_stream::stream! {
            while let Some(event) = response_rx.recv().await {
                match event.kind {
                    EventKind::Text { text, request } => {
                        let chunk = ChatCompletionChunk {
                            id: request.id().to_string(),
                            object: "chat.completion.chunk".to_string(),
                            created,
                            model: model_id.clone(),
                            choices: vec![ChatCompletionChunkChoice {
                                index: 0,
                                delta: ChatCompletionChunkDelta {
                                    role: None,
                                    content: Some(text),
                                },
                                finish_reason: None,
                            }],
                        };
                        match serde_json::to_string(&chunk) {

                            Ok(data) => yield Ok::<_, axum::BoxError>(SseEvent::default().data(data)),
                            Err(_) => break,
                        }
                    }
                    EventKind::Finish { request, reason } => {
                        let chunk = ChatCompletionChunk {
                            id: request.id().to_string(),
                            object: "chat.completion.chunk".to_string(),
                            created,
                            model: model_id.clone(),
                            choices: vec![ChatCompletionChunkChoice {
                                index: 0,
                                delta: ChatCompletionChunkDelta {
                                    role: None,
                                    content: None,
                                },
                                finish_reason: Some(format!("{:?}", reason)),
                            }],
                        };
                         match serde_json::to_string(&chunk) {
                            Ok(data) => yield Ok(SseEvent::default().data(data)),
                            Err(_) => break,
                        }

                        yield Ok(SseEvent::default().data("[DONE]"));
                        break;
                    }
                     EventKind::Error { message, .. } => {
                         let (status, body) = classify_chat_error(&message);
                         let err_payload = serde_json::json!({
                            "status": status.as_u16(),
                            "body": body
                         });
                         let data = serde_json::to_string(&err_payload)
                            .unwrap_or_else(|_| "{\"status\":500,\"body\":{\"error\":{\"message\":\"serialization error\",\"type\":\"server_error\",\"param\":null,\"code\":null}}}".to_string());
                         yield Ok(SseEvent::default().event("error").data(data));
                         break;
                     }
                     EventKind::Embedding { .. } => {}
                }
            }
        };

        Sse::new(stream)
            .keep_alive(axum::response::sse::KeepAlive::default())
            .into_response()
    } else {
        let mut full_text = String::new();
        let mut finish_reason = None;
        let mut req_id = String::new();

        while let Some(event) = response_rx.recv().await {
            match event.kind {
                EventKind::Text { text, request } => {
                    full_text.push_str(&text);
                    req_id = request.id().to_string();
                }
                EventKind::Finish { request, reason } => {
                    req_id = request.id().to_string();
                    finish_reason = Some(format!("{:?}", reason));
                    break;
                }
                EventKind::Error { message, .. } => {
                    let (status, body) = classify_chat_error(&message);
                    return (status, Json(body)).into_response();
                }
                EventKind::Embedding { .. } => {}
            }
        }

        let response = ChatCompletionResponse {
            id: req_id,
            object: "chat.completion".to_string(),
            created,
            model: model_id,
            choices: vec![ChatCompletionChoice {
                index: 0,
                message: ChatCompletionChoiceMessage {
                    role: "assistant".to_string(),
                    content: full_text,
                },
                finish_reason,
            }],
            usage: None,
        };
        Json(response).into_response()
    }
}

pub async fn list_models(State(state): State<Arc<AppState>>) -> Json<ModelList> {
    let model_id = state
        .config_name
        .clone()
        .or_else(|| state.model_name.clone())
        .unwrap_or_else(|| "unknown".to_string());
    Json(ModelList {
        object: "list".to_string(),
        data: vec![ModelInfo {
            id: model_id,
            object: "model".to_string(),
            created: 1677652288,
            owned_by: "system".to_string(),
        }],
    })
}

pub async fn monitor(State(state): State<Arc<AppState>>) -> Json<MonitorStats> {
    let sessions = state.kv_manager.sessions.read();
    let vram_count = sessions
        .values()
        .filter(|s| s.tier == crate::engine::kv_cache::CacheTier::Vram)
        .count();
    let ram_count = sessions
        .values()
        .filter(|s| s.tier == crate::engine::kv_cache::CacheTier::Ram)
        .count();
    let ram_usage_bytes: u64 = sessions
        .values()
        .filter_map(|s| s.ram_state.as_ref())
        .map(|v| v.len() as u64)
        .sum();
    drop(sessions);

    let stats = state.engine_stats.read();

    Json(MonitorStats {
        config_name: state
            .config_name
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        server_state: "running".to_string(),
        requests_processing: stats.requests_processing,
        requests_waiting: stats.requests_waiting,
        tokens_per_sec_total: stats.tokens_per_sec_total,
        tokens_per_sec_per_active: stats.tokens_per_sec_per_active,
        slots_usage: SlotsUsage {
            active: stats.slots_active,
            total: stats.slots_total,
        },
        memory: MemoryStats {
            ram_usage_bytes,
            vram_usage_bytes: None,
        },
        cache_stats: CacheStats {
            vram_sessions: vram_count,
            ram_sessions: ram_count,
        },
    })
}

pub async fn save_state(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<StateSaveRequest>,
) -> impl IntoResponse {
    let mut sessions = state.kv_manager.sessions.write();
    if let Some(session) = sessions.get_mut(&payload.session_id) {
        let path = paths::sessions_dir().join(format!("{}.bin", payload.template_id));
        let path = path.to_string_lossy().to_string();

        let _ = std::fs::create_dir_all("cache");

        session.pending_action = Some(crate::engine::kv_cache::Action::Save { path });
        (StatusCode::ACCEPTED, "State save requested")
    } else {
        (StatusCode::NOT_FOUND, "Session not found")
    }
}

pub async fn idle_state(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<StateIdleRequest>,
) -> impl IntoResponse {
    let mut sessions = state.kv_manager.sessions.write();
    if let Some(session) = sessions.get_mut(&payload.session_id) {
        session.pending_action = Some(crate::engine::kv_cache::Action::Idle);
        (StatusCode::ACCEPTED, "State idle requested")
    } else {
        (StatusCode::NOT_FOUND, "Session not found")
    }
}

pub async fn get_state(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let sessions = state.kv_manager.sessions.read();
    let available = if let Some(session) = sessions.get(&id) {
        let has_vram = session.tier == crate::engine::kv_cache::CacheTier::Vram
            && session.vram_seq_id.is_some();
        let has_ram = session.ram_state.is_some();
        let has_disk = session.disk_path.as_ref().is_some_and(|p| p.exists());
        has_vram || has_ram || has_disk
    } else {
        false
    };

    if available {
        Json(StateStatusResponse {
            session_id: id,
            exists: true,
        })
        .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(StateStatusResponse {
                session_id: id,
                exists: false,
            }),
        )
            .into_response()
    }
}

pub async fn delete_state(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mut sessions = state.kv_manager.sessions.write();
    if let Some(session) = sessions.get_mut(&id) {
        session.pending_action = Some(crate::engine::kv_cache::Action::Delete);
        (StatusCode::ACCEPTED, "State deletion requested")
    } else {
        (StatusCode::NOT_FOUND, "Session not found")
    }
}

pub async fn embeddings(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<EmbeddingRequest>,
) -> impl IntoResponse {
    let inputs = match &payload.input {
        EmbeddingInput::String(s) => vec![s.clone()],
        EmbeddingInput::Array(arr) => arr.clone(),
    };

    if inputs.is_empty() {
        return (StatusCode::BAD_REQUEST, "Input cannot be empty").into_response();
    }

    let mut data = Vec::new();

    for (i, input_text) in inputs.iter().enumerate() {
        let (response_tx, mut response_rx) = mpsc::unbounded_channel();
        let request_id = uuid::Uuid::new_v4().to_string();

        let mut params = RequestParams::default();
        params.id = request_id.clone();
        params.prompt = input_text.clone();
        params.embedding = true;

        let mut req = Request::new(params);
        req.response_tx = Some(response_tx);

        if state.engine_tx.send(req).await.is_err() {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Engine Unavailable").into_response();
        }

        let mut embedding: Option<Vec<f32>> = None;

        while let Some(event) = response_rx.recv().await {
            match event.kind {
                EventKind::Embedding { embedding: vec, .. } => {
                    embedding = Some(vec);
                }
                EventKind::Error { message, .. } => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "error": {
                                "message": message,
                                "type": "server_error",
                                "param": null,
                                "code": null
                            }
                        })),
                    )
                        .into_response();
                }
                EventKind::Finish { .. } => break,
                _ => {}
            }
        }

        if let Some(emb) = embedding {
            data.push(EmbeddingData {
                object: "embedding".to_string(),
                index: i,
                embedding: emb,
            });
        } else {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "message": "Failed to generate embedding (no embedding event received)",
                        "type": "server_error",
                        "param": null,
                        "code": null
                    }
                })),
            )
                .into_response();
        }
    }

    let response = EmbeddingResponse {
        object: "list".to_string(),
        data,
        model: payload.model,
        usage: EmbeddingUsage {
            prompt_tokens: 0,
            total_tokens: 0,
        },
    };

    Json(response).into_response()
}
