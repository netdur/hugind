use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event as SseEvent, Sse},
    },
};
use futures_util::stream::{FuturesUnordered, StreamExt};
use std::sync::Arc;
use tokio::sync::mpsc;

use std::time::{SystemTime, UNIX_EPOCH};

use crate::engine::request::{Request, RequestParams};
use crate::engine::types::EventKind;
use crate::llm::chat::{Message, apply_template};
use crate::llm::sampling::GrammarParams;
use crate::llm::tokenizer::Token;
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

const JSON_THINKING_GRAMMAR: &str = r#"
root ::= thinking-content "</think>" ws object

# Matches any characters up to </think> by just accepting wide ranges
thinking-content ::= ( [^<] | "<" [^/] | "</" [^t] | "</t" [^h] | "</th" [^i] | "</thi" [^n] | "</thin" [^k] | "</think" [^>] )*

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

const PLAIN_THINKING_GRAMMAR: &str = r#"
root ::= thinking-content "</think>" ws plain-text

# Matches any characters up to </think> by just accepting wide ranges
thinking-content ::= ( [^<] | "<" [^/] | "</" [^t] | "</t" [^h] | "</th" [^i] | "</thi" [^n] | "</thin" [^k] | "</think" [^>] )*

# Allow any non-NUL bytes after </think> as plain assistant output.
plain-text ::= [^\x00]*

# Optional space
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

/// Check if the system prompt should be injected.
/// Note: there is a small race window where a session could be created between
/// this check and when the engine processes the request. The worst case is
/// a redundant system prompt on one turn — acceptable for the simplicity.
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
            let is_new = !sessions.contains_key(id);
            // Also check if session exists but has no tokens yet (just registered)
            if !is_new {
                if let Some(session) = sessions.get(id) {
                    return session.tokens.is_empty();
                }
            }
            is_new
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

const MAX_IMAGE_BYTES: u64 = 100 * 1024 * 1024; // 100 MiB

async fn load_image_bytes(url: &str) -> Result<Vec<u8>, String> {
    if url.starts_with("data:") {
        let (_, data_part) = url
            .split_once("base64,")
            .ok_or("Invalid data URL (missing base64)".to_string())?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data_part.as_bytes())
            .map_err(|e| format!("Failed to decode base64 image: {}", e))?;
        if bytes.len() as u64 > MAX_IMAGE_BYTES {
            return Err(format!("Image too large ({} bytes, max {})", bytes.len(), MAX_IMAGE_BYTES));
        }
        return Ok(bytes);
    }

    if url.starts_with("http://") || url.starts_with("https://") {
        // Reject URLs pointing to private/link-local networks (SSRF protection)
        if let Some(host) = extract_host(url) {
            if let Ok(addr) = host.parse::<std::net::IpAddr>() {
                if addr.is_loopback() || is_private_ip(&addr) || is_link_local(&addr) {
                    return Err("Image URL must not point to private/internal networks".to_string());
                }
            }
            if host == "metadata.google.internal" || host.ends_with(".internal") {
                return Err("Image URL must not point to internal services".to_string());
            }
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::limited(3))
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

        // Check Content-Length before downloading
        if let Some(cl) = resp.content_length() {
            if cl > MAX_IMAGE_BYTES {
                return Err(format!("Image too large ({} bytes, max {})", cl, MAX_IMAGE_BYTES));
            }
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("Failed to read image bytes: {}", e))?;
        if bytes.len() as u64 > MAX_IMAGE_BYTES {
            return Err(format!("Image too large ({} bytes, max {})", bytes.len(), MAX_IMAGE_BYTES));
        }
        return Ok(bytes.to_vec());
    }

    Err("image_url must be a data URL or http(s) URL".to_string())
}

fn extract_host(url: &str) -> Option<String> {
    // Strip scheme
    let after_scheme = url.split("://").nth(1)?;
    // Take host:port part (before first /)
    let authority = after_scheme.split('/').next()?;
    // Strip port
    let host = if authority.starts_with('[') {
        // IPv6 in brackets
        authority.split(']').next().map(|s| s.trim_start_matches('['))
    } else {
        authority.rsplit_once(':').map(|(h, _)| h).or(Some(authority))
    };
    host.map(|h| h.to_string())
}

fn is_private_ip(addr: &std::net::IpAddr) -> bool {
    match addr {
        std::net::IpAddr::V4(v4) => v4.is_private(),
        std::net::IpAddr::V6(_) => false,
    }
}

fn is_link_local(addr: &std::net::IpAddr) -> bool {
    match addr {
        std::net::IpAddr::V4(v4) => v4.is_link_local(),
        std::net::IpAddr::V6(_) => false,
    }
}

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ChatCompletionRequest>,
) -> impl IntoResponse {
    if state.embeddings_enabled {
        return openai_error_response(
            StatusCode::BAD_REQUEST,
            "Chat completions are disabled when server is running in embedding mode",
            "invalid_request_error",
            Some("mode"),
            None,
        );
    }

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

    let thinking_budget_tokens = payload
        .thinking_budget_tokens
        .or(state.thinking_budget_tokens_default);
    let effective_enable_thinking = payload
        .enable_thinking
        .unwrap_or(state.enable_thinking_default);

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
    params.thinking_budget_tokens = thinking_budget_tokens.map(|b| b.min(1_000_000));
    params.enable_thinking = effective_enable_thinking;
    params.max_output_tokens = payload
        .max_tokens
        .map(|n| (n as i32).min(128_000))
        .unwrap_or(1024);
    params.sampling.temp = payload.temperature.unwrap_or(params.sampling.temp);
    params.sampling.top_p = payload.top_p.unwrap_or(params.sampling.top_p);

    if let Some(fp) = payload.frequency_penalty {
        params.sampling.penalty_repeat = 1.0 + fp.max(0.0);
    }

    if let Some(fmt) = &payload.response_format {
        if fmt.format_type == "json_object" {
            if effective_enable_thinking {
                let mut grammar_str = JSON_THINKING_GRAMMAR.to_string();
                if let Some(budget) = thinking_budget_tokens {
                    // Inject the prefix into the root rule
                    grammar_str = grammar_str.replace(
                        "root ::= thinking-content \"</think>\" ws object",
                        &format!("root ::= \"(max thinking budget {} tokens)\\n\" thinking-content \"</think>\" ws object", budget)
                    );
                }
                params.sampling.grammar = Some(GrammarParams {
                    grammar: grammar_str,
                    root: "root".to_string(),
                });
            } else {
                params.sampling.grammar = Some(GrammarParams {
                    grammar: JSON_GRAMMAR.to_string(),
                    root: "root".to_string(),
                });
            }
        }
    } else if effective_enable_thinking {
        // Plain-text mode with thinking enabled: enforce an eventual </think>.
        let mut grammar_str = PLAIN_THINKING_GRAMMAR.to_string();
        if let Some(budget) = thinking_budget_tokens {
            grammar_str = grammar_str.replace(
                "root ::= thinking-content \"</think>\" ws plain-text",
                &format!(
                    "root ::= \"(max thinking budget {} tokens)\\n\" thinking-content \"</think>\" ws plain-text",
                    budget
                ),
            );
        }
        params.sampling.grammar = Some(GrammarParams {
            grammar: grammar_str,
            root: "root".to_string(),
        });
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

fn openai_error_payload(
    message: impl Into<String>,
    error_type: &'static str,
    param: Option<&str>,
    code: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "error": {
            "message": message.into(),
            "type": error_type,
            "param": param,
            "code": code,
        }
    })
}

fn openai_error_response(
    status: StatusCode,
    message: impl Into<String>,
    error_type: &'static str,
    param: Option<&str>,
    code: Option<&str>,
) -> Response {
    (
        status,
        Json(openai_error_payload(message, error_type, param, code)),
    )
        .into_response()
}

fn invalid_embedding_request(message: impl Into<String>, param: Option<&str>) -> Response {
    openai_error_response(
        StatusCode::BAD_REQUEST,
        message,
        "invalid_request_error",
        param,
        None,
    )
}

fn encode_embedding_base64(values: &[f32]) -> String {
    let mut bytes = Vec::with_capacity(values.len() * std::mem::size_of::<f32>());
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn parse_token_input(token_ids: &[i64], input_index: usize) -> Result<Vec<Token>, Response> {
    if token_ids.is_empty() {
        return Err(invalid_embedding_request(
            format!("input[{}] cannot be empty", input_index),
            Some("input"),
        ));
    }

    let mut tokens = Vec::with_capacity(token_ids.len());
    for (token_index, token_id) in token_ids.iter().enumerate() {
        if !(0..=i32::MAX as i64).contains(token_id) {
            return Err(invalid_embedding_request(
                format!(
                    "input[{}][{}] must be between 0 and {}",
                    input_index,
                    token_index,
                    i32::MAX
                ),
                Some("input"),
            ));
        }
        tokens.push(Token(*token_id as i32));
    }
    Ok(tokens)
}

#[derive(Debug)]
struct PreparedEmbeddingInput {
    index: usize,
    prompt: String,
    prompt_tokens_override: Option<Vec<Token>>,
}

fn prepare_embedding_inputs(
    input: EmbeddingInput,
) -> Result<Vec<PreparedEmbeddingInput>, Response> {
    let mut prepared = Vec::new();

    match input {
        EmbeddingInput::String(value) => {
            if value.is_empty() {
                return Err(invalid_embedding_request(
                    "Input cannot be empty",
                    Some("input"),
                ));
            }
            prepared.push(PreparedEmbeddingInput {
                index: 0,
                prompt: value,
                prompt_tokens_override: None,
            });
        }
        EmbeddingInput::StringArray(values) => {
            if values.is_empty() {
                return Err(invalid_embedding_request(
                    "Input cannot be empty",
                    Some("input"),
                ));
            }

            for (i, value) in values.into_iter().enumerate() {
                if value.is_empty() {
                    return Err(invalid_embedding_request(
                        format!("input[{}] cannot be empty", i),
                        Some("input"),
                    ));
                }
                prepared.push(PreparedEmbeddingInput {
                    index: i,
                    prompt: value,
                    prompt_tokens_override: None,
                });
            }
        }
        EmbeddingInput::Tokens(token_ids) => {
            let tokens = parse_token_input(&token_ids, 0)?;
            prepared.push(PreparedEmbeddingInput {
                index: 0,
                prompt: String::new(),
                prompt_tokens_override: Some(tokens),
            });
        }
        EmbeddingInput::TokenArray(token_batches) => {
            if token_batches.is_empty() {
                return Err(invalid_embedding_request(
                    "Input cannot be empty",
                    Some("input"),
                ));
            }

            for (i, token_ids) in token_batches.into_iter().enumerate() {
                let tokens = parse_token_input(&token_ids, i)?;
                prepared.push(PreparedEmbeddingInput {
                    index: i,
                    prompt: String::new(),
                    prompt_tokens_override: Some(tokens),
                });
            }
        }
    }

    Ok(prepared)
}

async fn collect_embedding(
    mut response_rx: mpsc::UnboundedReceiver<crate::engine::types::Event>,
) -> Result<(Vec<f32>, u32), Response> {
    let mut embedding: Option<Vec<f32>> = None;
    let mut prompt_tokens: u32 = 0;

    while let Some(event) = response_rx.recv().await {
        match event.kind {
            EventKind::Embedding {
                embedding: vector,
                prompt_tokens: used,
                ..
            } => {
                embedding = Some(vector);
                prompt_tokens = used;
            }
            EventKind::Error { message, .. } => {
                return Err(openai_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    message,
                    "server_error",
                    None,
                    None,
                ));
            }
            EventKind::Finish { .. } => break,
            _ => {}
        }
    }

    let embedding = embedding.ok_or_else(|| {
        openai_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to generate embedding (no embedding event received)",
            "server_error",
            None,
            None,
        )
    })?;

    Ok((embedding, prompt_tokens))
}

pub async fn embeddings(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<EmbeddingRequest>,
) -> impl IntoResponse {
    if !state.embeddings_enabled {
        return invalid_embedding_request(
            "Embeddings endpoint is disabled when server is not running in embedding mode",
            Some("mode"),
        );
    }

    let encoding_format = payload
        .encoding_format
        .as_deref()
        .unwrap_or("float")
        .to_ascii_lowercase();
    if encoding_format != "float" && encoding_format != "base64" {
        return invalid_embedding_request(
            "encoding_format must be either \"float\" or \"base64\"",
            Some("encoding_format"),
        );
    }

    let prepared_inputs = match prepare_embedding_inputs(payload.input) {
        Ok(inputs) => inputs,
        Err(resp) => return resp,
    };

    let mut submitted = Vec::with_capacity(prepared_inputs.len());
    for input in prepared_inputs {
        let (response_tx, response_rx) = mpsc::unbounded_channel();
        let request_id = uuid::Uuid::new_v4().to_string();

        let mut params = RequestParams::default();
        params.id = request_id;
        params.prompt = input.prompt;
        params.prompt_tokens_override = input.prompt_tokens_override;
        params.embedding = true;

        let mut req = Request::new(params);
        req.response_tx = Some(response_tx);

        if state.engine_tx.send(req).await.is_err() {
            return openai_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Engine unavailable",
                "server_error",
                None,
                None,
            );
        }

        submitted.push((input.index, response_rx));
    }

    let mut pending = FuturesUnordered::new();
    for (index, response_rx) in submitted {
        pending.push(async move {
            let (embedding, prompt_tokens) = collect_embedding(response_rx).await?;
            Ok::<(usize, Vec<f32>, u32), Response>((index, embedding, prompt_tokens))
        });
    }

    let mut results = Vec::new();
    while let Some(result) = pending.next().await {
        match result {
            Ok(item) => results.push(item),
            Err(resp) => return resp,
        }
    }

    results.sort_by_key(|(index, _, _)| *index);

    let prompt_tokens_sum_u64: u64 = results.iter().map(|(_, _, n)| *n as u64).sum();
    let prompt_tokens_sum = prompt_tokens_sum_u64.min(u32::MAX as u64) as u32;

    let mut data = Vec::with_capacity(results.len());
    for (index, embedding, _) in results {
        let embedding = if encoding_format == "base64" {
            EmbeddingVector::Base64(encode_embedding_base64(&embedding))
        } else {
            EmbeddingVector::Float(embedding)
        };

        data.push(EmbeddingData {
            object: "embedding".to_string(),
            index,
            embedding,
        });
    }

    Json(EmbeddingResponse {
        object: "list".to_string(),
        data,
        model: payload.model,
        usage: EmbeddingUsage {
            prompt_tokens: prompt_tokens_sum,
            total_tokens: prompt_tokens_sum,
        },
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::EngineStats;
    use crate::engine::kv_cache::{Action, CacheTier, KvCacheManager, Session};
    use crate::engine::request::Request as EngineRequest;
    use crate::engine::types::{Event, EventKind, RequestHandle, StopReason};
    use crate::llm::model::Model;
    use crate::llm::sampling::SamplingConfig;
    use axum::body::to_bytes;
    use axum::response::Response;
    use parking_lot::RwLock;
    use std::time::Instant;

    fn make_test_state() -> Arc<AppState> {
        let (engine_tx, _engine_rx) = tokio::sync::mpsc::channel(8);
        Arc::new(AppState {
            engine_tx,
            kv_manager: Arc::new(KvCacheManager::new(false)),
            engine_stats: Arc::new(RwLock::new(EngineStats::default())),
            model: Arc::new(Model::dummy()),
            model_name: Some("test-model".to_string()),
            config_name: Some("test-config".to_string()),
            api_key: None,
            embeddings_enabled: false,
            enable_thinking_default: false,
            thinking_budget_tokens_default: None,
            sampling_defaults: SamplingConfig::default(),
            system_prompt: None,
        })
    }

    fn make_test_state_with_engine_rx()
    -> (Arc<AppState>, tokio::sync::mpsc::Receiver<EngineRequest>) {
        let (engine_tx, engine_rx) = tokio::sync::mpsc::channel(8);
        (
            Arc::new(AppState {
                engine_tx,
                kv_manager: Arc::new(KvCacheManager::new(false)),
                engine_stats: Arc::new(RwLock::new(EngineStats::default())),
                model: Arc::new(Model::dummy()),
                model_name: Some("test-model".to_string()),
                config_name: Some("test-config".to_string()),
                api_key: None,
                embeddings_enabled: false,
                enable_thinking_default: false,
                thinking_budget_tokens_default: None,
                sampling_defaults: SamplingConfig::default(),
                system_prompt: None,
            }),
            engine_rx,
        )
    }

    fn make_embeddings_state() -> Arc<AppState> {
        let (engine_tx, _engine_rx) = tokio::sync::mpsc::channel(8);
        Arc::new(AppState {
            engine_tx,
            kv_manager: Arc::new(KvCacheManager::new(false)),
            engine_stats: Arc::new(RwLock::new(EngineStats::default())),
            model: Arc::new(Model::dummy()),
            model_name: Some("test-model".to_string()),
            config_name: Some("test-config".to_string()),
            api_key: None,
            embeddings_enabled: true,
            enable_thinking_default: false,
            thinking_budget_tokens_default: None,
            sampling_defaults: SamplingConfig::default(),
            system_prompt: None,
        })
    }

    fn make_embeddings_state_with_engine_rx()
    -> (Arc<AppState>, tokio::sync::mpsc::Receiver<EngineRequest>) {
        let (engine_tx, engine_rx) = tokio::sync::mpsc::channel(8);
        (
            Arc::new(AppState {
                engine_tx,
                kv_manager: Arc::new(KvCacheManager::new(false)),
                engine_stats: Arc::new(RwLock::new(EngineStats::default())),
                model: Arc::new(Model::dummy()),
                model_name: Some("test-model".to_string()),
                config_name: Some("test-config".to_string()),
                api_key: None,
                embeddings_enabled: true,
                enable_thinking_default: false,
                thinking_budget_tokens_default: None,
                sampling_defaults: SamplingConfig::default(),
                system_prompt: None,
            }),
            engine_rx,
        )
    }

    fn make_session(id: &str, tier: CacheTier) -> Session {
        Session {
            id: id.to_string(),
            tier,
            last_used: Instant::now(),
            ram_state: None,
            ram_kv_head: None,
            disk_path: None,
            pending_action: None,
            tokens: vec![],
            n_keep: 0,
            vram_seq_id: None,
            kv_head: 0,
        }
    }

    async fn response_body_text(resp: Response) -> String {
        let bytes = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        String::from_utf8_lossy(&bytes).to_string()
    }

    #[test]
    fn extracts_headers_and_boolean_flags() {
        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", "req-1".parse().expect("header"));
        headers.insert("x-session-id", "sess-1".parse().expect("header"));
        headers.insert("x-flag", "TrUe".parse().expect("header"));

        assert_eq!(
            extract_nonempty_header(&headers, "x-session-id"),
            Some("sess-1".to_string())
        );
        assert_eq!(extract_session_id(&headers), Some("sess-1".to_string()));
        assert!(parse_bool_header(&headers, "x-flag"));
        assert!(!parse_bool_header(&headers, "missing"));
    }

    #[test]
    fn maps_context_shift_error_to_payload_too_large() {
        let (status, body) =
            classify_chat_error("Context shift unsupported by backend for this model");
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            body["error"]["code"].as_str(),
            Some("context_shift_unsupported")
        );
    }

    #[test]
    fn should_apply_system_prompt_for_fresh_or_new_sessions() {
        let state = make_test_state();
        assert!(should_apply_system_prompt(state.as_ref(), None, true));
        assert!(should_apply_system_prompt(
            state.as_ref(),
            Some("new-session"),
            false
        ));

        // Session exists but has no tokens — should still apply system prompt
        let mut session = make_session("empty-session", CacheTier::Vram);
        session.vram_seq_id = Some(1);
        state
            .kv_manager
            .sessions
            .write()
            .insert("empty-session".to_string(), session);
        assert!(should_apply_system_prompt(
            state.as_ref(),
            Some("empty-session"),
            false
        ));

        // Session with tokens — should NOT apply system prompt
        let mut active_session = make_session("active-session", CacheTier::Vram);
        active_session.vram_seq_id = Some(2);
        active_session.tokens = vec![Token(1), Token(2), Token(3)];
        state
            .kv_manager
            .sessions
            .write()
            .insert("active-session".to_string(), active_session);
        assert!(!should_apply_system_prompt(
            state.as_ref(),
            Some("active-session"),
            false
        ));
    }

    #[tokio::test]
    async fn load_image_bytes_accepts_data_urls() {
        let bytes = load_image_bytes("data:image/png;base64,aGVsbG8=")
            .await
            .expect("decode base64");
        assert_eq!(bytes, b"hello");
    }

    #[tokio::test]
    async fn load_image_bytes_rejects_unsupported_scheme() {
        let err = load_image_bytes("file:///tmp/test.png")
            .await
            .expect_err("should fail");
        assert!(err.contains("data URL or http(s) URL"));
    }

    #[tokio::test]
    async fn monitor_reports_engine_and_cache_snapshot() {
        let state = make_test_state();
        {
            let mut sessions = state.kv_manager.sessions.write();
            let mut vram = make_session("vram", CacheTier::Vram);
            vram.vram_seq_id = Some(7);
            sessions.insert("vram".to_string(), vram);
            let mut ram = make_session("ram", CacheTier::Ram);
            ram.ram_state = Some(vec![1, 2, 3, 4, 5]);
            sessions.insert("ram".to_string(), ram);
        }
        {
            let mut stats = state.engine_stats.write();
            stats.requests_processing = 2;
            stats.requests_waiting = 3;
            stats.slots_active = 1;
            stats.slots_total = 4;
            stats.tokens_per_sec_total = 12.5;
            stats.tokens_per_sec_per_active = 12.5;
        }

        let Json(snapshot) = monitor(State(state)).await;
        assert_eq!(snapshot.requests_processing, 2);
        assert_eq!(snapshot.requests_waiting, 3);
        assert_eq!(snapshot.slots_usage.active, 1);
        assert_eq!(snapshot.slots_usage.total, 4);
        assert_eq!(snapshot.cache_stats.vram_sessions, 1);
        assert_eq!(snapshot.cache_stats.ram_sessions, 1);
        assert_eq!(snapshot.memory.ram_usage_bytes, 5);
    }

    #[tokio::test]
    async fn save_and_idle_state_update_pending_action() {
        let state = make_test_state();
        state
            .kv_manager
            .sessions
            .write()
            .insert("s1".to_string(), make_session("s1", CacheTier::Vram));

        let save_resp = save_state(
            State(state.clone()),
            Json(StateSaveRequest {
                session_id: "s1".to_string(),
                template_id: "tmpl".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(save_resp.status(), StatusCode::ACCEPTED);

        {
            let sessions = state.kv_manager.sessions.read();
            let pending = &sessions.get("s1").expect("session").pending_action;
            match pending {
                Some(Action::Save { path }) => assert!(path.ends_with("tmpl.bin")),
                _ => panic!("expected save action"),
            }
        }

        let idle_resp = idle_state(
            State(state.clone()),
            Json(StateIdleRequest {
                session_id: "s1".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(idle_resp.status(), StatusCode::ACCEPTED);

        let sessions = state.kv_manager.sessions.read();
        let pending = &sessions.get("s1").expect("session").pending_action;
        assert!(matches!(pending, Some(Action::Idle)));
    }

    #[tokio::test]
    async fn save_state_returns_not_found_for_unknown_session() {
        let state = make_test_state();
        let resp = save_state(
            State(state),
            Json(StateSaveRequest {
                session_id: "missing".to_string(),
                template_id: "tmpl".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_state_reports_exists_and_missing() {
        let state = make_test_state();
        let mut session = make_session("known", CacheTier::Vram);
        session.vram_seq_id = Some(5);
        state
            .kv_manager
            .sessions
            .write()
            .insert("known".to_string(), session);

        let exists_resp = get_state(State(state.clone()), Path("known".to_string()))
            .await
            .into_response();
        assert_eq!(exists_resp.status(), StatusCode::OK);
        let exists_body = response_body_text(exists_resp).await;
        let exists: StateStatusResponse = serde_json::from_str(&exists_body).expect("json body");
        assert!(exists.exists);

        let missing_resp = get_state(State(state), Path("missing".to_string()))
            .await
            .into_response();
        assert_eq!(missing_resp.status(), StatusCode::NOT_FOUND);
        let missing_body = response_body_text(missing_resp).await;
        let missing: StateStatusResponse = serde_json::from_str(&missing_body).expect("json body");
        assert!(!missing.exists);
    }

    #[tokio::test]
    async fn delete_state_sets_pending_action_or_returns_not_found() {
        let state = make_test_state();
        state.kv_manager.sessions.write().insert(
            "delete-me".to_string(),
            make_session("delete-me", CacheTier::Vram),
        );

        let ok_resp = delete_state(State(state.clone()), Path("delete-me".to_string()))
            .await
            .into_response();
        assert_eq!(ok_resp.status(), StatusCode::ACCEPTED);
        {
            let sessions = state.kv_manager.sessions.read();
            let pending = &sessions.get("delete-me").expect("session").pending_action;
            assert!(matches!(pending, Some(Action::Delete)));
        }

        let not_found = delete_state(State(state), Path("missing".to_string()))
            .await
            .into_response();
        assert_eq!(not_found.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn chat_completions_rejects_empty_messages() {
        let state = make_test_state();
        let resp = chat_completions(
            State(state),
            HeaderMap::new(),
            Json(ChatCompletionRequest {
                model: "test".to_string(),
                messages: vec![],
                stream: Some(false),
                max_tokens: None,
                temperature: None,
                top_p: None,
                frequency_penalty: None,
                presence_penalty: None,
                stop: None,
                response_format: None,
                enable_thinking: None,
                thinking_budget_tokens: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = response_body_text(resp).await;
        assert!(body.contains("Messages cannot be empty"));
    }

    #[tokio::test]
    async fn chat_completions_applies_defaults_and_returns_non_stream_response() {
        let (state, mut engine_rx) = make_test_state_with_engine_rx();
        let request = ChatCompletionRequest {
            model: "test".to_string(),
            messages: vec![ChatCompletionMessage {
                role: "user".to_string(),
                content: Content::Text("hello".to_string()),
                name: None,
            }],
            stream: Some(false),
            max_tokens: None,
            temperature: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            response_format: Some(ResponseFormat {
                format_type: "json_object".to_string(),
            }),
            enable_thinking: Some(true),
            thinking_budget_tokens: Some(77),
        };

        let route_task = tokio::spawn(async move {
            chat_completions(State(state), HeaderMap::new(), Json(request))
                .await
                .into_response()
        });

        let mut engine_req = engine_rx.recv().await.expect("engine request");
        assert_eq!(engine_req.params.max_output_tokens, 1024);
        assert_eq!(
            engine_req.params.sampling.temp,
            SamplingConfig::default().temp
        );
        assert_eq!(
            engine_req.params.sampling.top_p,
            SamplingConfig::default().top_p
        );
        assert_eq!(engine_req.params.enable_thinking, true);
        assert_eq!(engine_req.params.thinking_budget_tokens, Some(77));
        let grammar = engine_req
            .params
            .sampling
            .grammar
            .as_ref()
            .expect("grammar should be set");
        assert_eq!(grammar.root, "root");
        assert!(grammar.grammar.contains("(max thinking budget 77 tokens)"));

        let response_tx = engine_req.response_tx.take().expect("response channel");
        let handle =
            RequestHandle::new(engine_req.params.id.clone(), engine_req.cancel_flag.clone());
        response_tx
            .send(Event {
                id: engine_req.params.id.clone(),
                kind: EventKind::Text {
                    text: "assistant reply".to_string(),
                    request: handle.clone(),
                },
            })
            .expect("send text");
        response_tx
            .send(Event {
                id: engine_req.params.id.clone(),
                kind: EventKind::Finish {
                    request: handle,
                    reason: StopReason::Eos,
                },
            })
            .expect("send finish");

        let resp = route_task.await.expect("task join");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_text(resp).await;
        let json: serde_json::Value = serde_json::from_str(&body).expect("json body");
        assert_eq!(json["choices"][0]["message"]["content"], "assistant reply");
    }

    #[tokio::test]
    async fn chat_completions_streams_sse_chunks_and_done_marker() {
        let (state, mut engine_rx) = make_test_state_with_engine_rx();
        let request = ChatCompletionRequest {
            model: "test".to_string(),
            messages: vec![ChatCompletionMessage {
                role: "user".to_string(),
                content: Content::Text("hello".to_string()),
                name: None,
            }],
            stream: Some(true),
            max_tokens: Some(12),
            temperature: Some(0.3),
            top_p: Some(0.8),
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            response_format: None,
            enable_thinking: Some(false),
            thinking_budget_tokens: None,
        };

        let route_task = tokio::spawn(async move {
            chat_completions(State(state), HeaderMap::new(), Json(request))
                .await
                .into_response()
        });

        let mut engine_req = engine_rx.recv().await.expect("engine request");
        assert_eq!(engine_req.params.max_output_tokens, 12);
        assert_eq!(engine_req.params.sampling.temp, 0.3);
        assert_eq!(engine_req.params.sampling.top_p, 0.8);

        let response_tx = engine_req.response_tx.take().expect("response channel");
        let handle =
            RequestHandle::new(engine_req.params.id.clone(), engine_req.cancel_flag.clone());
        response_tx
            .send(Event {
                id: engine_req.params.id.clone(),
                kind: EventKind::Text {
                    text: "delta".to_string(),
                    request: handle.clone(),
                },
            })
            .expect("send text");
        response_tx
            .send(Event {
                id: engine_req.params.id.clone(),
                kind: EventKind::Finish {
                    request: handle,
                    reason: StopReason::Eos,
                },
            })
            .expect("send finish");

        let resp = route_task.await.expect("task join");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_text(resp).await;
        assert!(body.contains("data:"));
        assert!(body.contains("\"content\":\"delta\""));
        assert!(body.contains("[DONE]"));
    }

    #[tokio::test]
    async fn chat_completions_applies_frequency_penalty_to_repeat_penalty() {
        let (state, mut engine_rx) = make_test_state_with_engine_rx();
        let request = ChatCompletionRequest {
            model: "test".to_string(),
            messages: vec![ChatCompletionMessage {
                role: "user".to_string(),
                content: Content::Text("hello".to_string()),
                name: None,
            }],
            stream: Some(false),
            max_tokens: None,
            temperature: None,
            top_p: None,
            frequency_penalty: Some(0.7),
            presence_penalty: None,
            stop: None,
            response_format: None,
            enable_thinking: Some(false),
            thinking_budget_tokens: None,
        };

        let route_task = tokio::spawn(async move {
            chat_completions(State(state), HeaderMap::new(), Json(request))
                .await
                .into_response()
        });

        let mut engine_req = engine_rx.recv().await.expect("engine request");
        assert!((engine_req.params.sampling.penalty_repeat - 1.7).abs() < 1e-6);
        let response_tx = engine_req.response_tx.take().expect("response channel");
        let handle =
            RequestHandle::new(engine_req.params.id.clone(), engine_req.cancel_flag.clone());
        response_tx
            .send(Event {
                id: engine_req.params.id.clone(),
                kind: EventKind::Finish {
                    request: handle,
                    reason: StopReason::Eos,
                },
            })
            .expect("send finish");

        let resp = route_task.await.expect("task join");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn chat_completions_clamps_negative_frequency_penalty() {
        let (state, mut engine_rx) = make_test_state_with_engine_rx();
        let request = ChatCompletionRequest {
            model: "test".to_string(),
            messages: vec![ChatCompletionMessage {
                role: "user".to_string(),
                content: Content::Text("hello".to_string()),
                name: None,
            }],
            stream: Some(false),
            max_tokens: None,
            temperature: None,
            top_p: None,
            frequency_penalty: Some(-3.0),
            presence_penalty: None,
            stop: None,
            response_format: None,
            enable_thinking: Some(false),
            thinking_budget_tokens: None,
        };

        let route_task = tokio::spawn(async move {
            chat_completions(State(state), HeaderMap::new(), Json(request))
                .await
                .into_response()
        });

        let mut engine_req = engine_rx.recv().await.expect("engine request");
        assert!((engine_req.params.sampling.penalty_repeat - 1.0).abs() < 1e-6);
        let response_tx = engine_req.response_tx.take().expect("response channel");
        let handle =
            RequestHandle::new(engine_req.params.id.clone(), engine_req.cancel_flag.clone());
        response_tx
            .send(Event {
                id: engine_req.params.id.clone(),
                kind: EventKind::Finish {
                    request: handle,
                    reason: StopReason::Eos,
                },
            })
            .expect("send finish");

        let resp = route_task.await.expect("task join");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn chat_completions_sets_json_grammar_without_thinking() {
        let (state, mut engine_rx) = make_test_state_with_engine_rx();
        let request = ChatCompletionRequest {
            model: "test".to_string(),
            messages: vec![ChatCompletionMessage {
                role: "user".to_string(),
                content: Content::Text("hello".to_string()),
                name: None,
            }],
            stream: Some(false),
            max_tokens: None,
            temperature: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            response_format: Some(ResponseFormat {
                format_type: "json_object".to_string(),
            }),
            enable_thinking: Some(false),
            thinking_budget_tokens: None,
        };

        let route_task = tokio::spawn(async move {
            chat_completions(State(state), HeaderMap::new(), Json(request))
                .await
                .into_response()
        });

        let mut engine_req = engine_rx.recv().await.expect("engine request");
        let grammar = engine_req
            .params
            .sampling
            .grammar
            .as_ref()
            .expect("grammar should be set");
        assert_eq!(grammar.root, "root");
        assert_eq!(grammar.grammar, JSON_GRAMMAR);

        let response_tx = engine_req.response_tx.take().expect("response channel");
        let handle =
            RequestHandle::new(engine_req.params.id.clone(), engine_req.cancel_flag.clone());
        response_tx
            .send(Event {
                id: engine_req.params.id.clone(),
                kind: EventKind::Finish {
                    request: handle,
                    reason: StopReason::Eos,
                },
            })
            .expect("send finish");

        let resp = route_task.await.expect("task join");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn chat_completions_sets_plain_thinking_grammar_with_budget() {
        let (state, mut engine_rx) = make_test_state_with_engine_rx();
        let request = ChatCompletionRequest {
            model: "test".to_string(),
            messages: vec![ChatCompletionMessage {
                role: "user".to_string(),
                content: Content::Text("hello".to_string()),
                name: None,
            }],
            stream: Some(false),
            max_tokens: None,
            temperature: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            response_format: None,
            enable_thinking: Some(true),
            thinking_budget_tokens: Some(9),
        };

        let route_task = tokio::spawn(async move {
            chat_completions(State(state), HeaderMap::new(), Json(request))
                .await
                .into_response()
        });

        let mut engine_req = engine_rx.recv().await.expect("engine request");
        let grammar = engine_req
            .params
            .sampling
            .grammar
            .as_ref()
            .expect("grammar should be set");
        assert_eq!(grammar.root, "root");
        assert!(grammar.grammar.contains("plain-text"));
        assert!(grammar.grammar.contains("(max thinking budget 9 tokens)"));

        let response_tx = engine_req.response_tx.take().expect("response channel");
        let handle =
            RequestHandle::new(engine_req.params.id.clone(), engine_req.cancel_flag.clone());
        response_tx
            .send(Event {
                id: engine_req.params.id.clone(),
                kind: EventKind::Finish {
                    request: handle,
                    reason: StopReason::Eos,
                },
            })
            .expect("send finish");

        let resp = route_task.await.expect("task join");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn chat_completions_non_thinking_plain_mode_leaves_grammar_unset() {
        let (state, mut engine_rx) = make_test_state_with_engine_rx();
        let request = ChatCompletionRequest {
            model: "test".to_string(),
            messages: vec![ChatCompletionMessage {
                role: "user".to_string(),
                content: Content::Text("hello".to_string()),
                name: None,
            }],
            stream: Some(false),
            max_tokens: None,
            temperature: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            response_format: None,
            enable_thinking: Some(false),
            thinking_budget_tokens: None,
        };

        let route_task = tokio::spawn(async move {
            chat_completions(State(state), HeaderMap::new(), Json(request))
                .await
                .into_response()
        });

        let mut engine_req = engine_rx.recv().await.expect("engine request");
        assert!(engine_req.params.sampling.grammar.is_none());

        let response_tx = engine_req.response_tx.take().expect("response channel");
        let handle =
            RequestHandle::new(engine_req.params.id.clone(), engine_req.cancel_flag.clone());
        response_tx
            .send(Event {
                id: engine_req.params.id.clone(),
                kind: EventKind::Finish {
                    request: handle,
                    reason: StopReason::Eos,
                },
            })
            .expect("send finish");

        let resp = route_task.await.expect("task join");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn chat_completions_non_stream_error_maps_to_http_payload() {
        let (state, mut engine_rx) = make_test_state_with_engine_rx();
        let request = ChatCompletionRequest {
            model: "test".to_string(),
            messages: vec![ChatCompletionMessage {
                role: "user".to_string(),
                content: Content::Text("hello".to_string()),
                name: None,
            }],
            stream: Some(false),
            max_tokens: None,
            temperature: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            response_format: None,
            enable_thinking: Some(false),
            thinking_budget_tokens: None,
        };

        let route_task = tokio::spawn(async move {
            chat_completions(State(state), HeaderMap::new(), Json(request))
                .await
                .into_response()
        });

        let mut engine_req = engine_rx.recv().await.expect("engine request");
        let response_tx = engine_req.response_tx.take().expect("response channel");
        let handle =
            RequestHandle::new(engine_req.params.id.clone(), engine_req.cancel_flag.clone());
        response_tx
            .send(Event {
                id: engine_req.params.id.clone(),
                kind: EventKind::Error {
                    message: "Context shift unsupported by backend for this model".to_string(),
                    request: handle,
                },
            })
            .expect("send error");

        let resp = route_task.await.expect("task join");
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body = response_body_text(resp).await;
        assert!(body.contains("context_shift_unsupported"));
    }

    #[tokio::test]
    async fn chat_completions_stream_error_emits_sse_error_event_payload() {
        let (state, mut engine_rx) = make_test_state_with_engine_rx();
        let request = ChatCompletionRequest {
            model: "test".to_string(),
            messages: vec![ChatCompletionMessage {
                role: "user".to_string(),
                content: Content::Text("hello".to_string()),
                name: None,
            }],
            stream: Some(true),
            max_tokens: None,
            temperature: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            response_format: None,
            enable_thinking: Some(false),
            thinking_budget_tokens: None,
        };

        let route_task = tokio::spawn(async move {
            chat_completions(State(state), HeaderMap::new(), Json(request))
                .await
                .into_response()
        });

        let mut engine_req = engine_rx.recv().await.expect("engine request");
        let response_tx = engine_req.response_tx.take().expect("response channel");
        let handle =
            RequestHandle::new(engine_req.params.id.clone(), engine_req.cancel_flag.clone());
        response_tx
            .send(Event {
                id: engine_req.params.id.clone(),
                kind: EventKind::Error {
                    message: "boom".to_string(),
                    request: handle,
                },
            })
            .expect("send error");

        let resp = route_task.await.expect("task join");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_text(resp).await;
        assert!(body.contains("event: error"));
        assert!(body.contains("\"status\":500"));
        assert!(body.contains("\"server_error\""));
    }

    #[tokio::test]
    async fn chat_completions_rejects_when_embeddings_mode_enabled() {
        let state = make_embeddings_state();
        let request = ChatCompletionRequest {
            model: "test".to_string(),
            messages: vec![ChatCompletionMessage {
                role: "user".to_string(),
                content: Content::Text("hello".to_string()),
                name: None,
            }],
            stream: Some(false),
            max_tokens: None,
            temperature: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            response_format: None,
            enable_thinking: Some(false),
            thinking_budget_tokens: None,
        };

        let resp = chat_completions(State(state), HeaderMap::new(), Json(request))
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = response_body_text(resp).await;
        let json: serde_json::Value = serde_json::from_str(&body).expect("json body");
        assert_eq!(json["error"]["type"], "invalid_request_error");
        assert_eq!(json["error"]["param"], "mode");
    }

    #[tokio::test]
    async fn embeddings_submits_batch_and_preserves_index_order() {
        let (state, mut engine_rx) = make_embeddings_state_with_engine_rx();
        let request = EmbeddingRequest {
            model: "test".to_string(),
            input: EmbeddingInput::StringArray(vec!["first".to_string(), "second".to_string()]),
            encoding_format: Some("float".to_string()),
            dimensions: None,
            user: None,
        };

        let route_task = tokio::spawn(async move {
            embeddings(State(state), Json(request))
                .await
                .into_response()
        });

        let mut req1 = engine_rx.recv().await.expect("first request");
        let mut req2 = engine_rx.recv().await.expect("second request");

        assert!(req1.params.embedding);
        assert!(req2.params.embedding);

        let tx1 = req1.response_tx.take().expect("response channel");
        let tx2 = req2.response_tx.take().expect("response channel");
        let handle1 = RequestHandle::new(req1.params.id.clone(), req1.cancel_flag.clone());
        let handle2 = RequestHandle::new(req2.params.id.clone(), req2.cancel_flag.clone());

        tx2.send(Event {
            id: req2.params.id.clone(),
            kind: EventKind::Embedding {
                embedding: vec![0.21, 0.22],
                prompt_tokens: 7,
                request: handle2.clone(),
            },
        })
        .expect("send embedding");
        tx2.send(Event {
            id: req2.params.id.clone(),
            kind: EventKind::Finish {
                request: handle2,
                reason: StopReason::Eos,
            },
        })
        .expect("send finish");

        tx1.send(Event {
            id: req1.params.id.clone(),
            kind: EventKind::Embedding {
                embedding: vec![0.11, 0.12],
                prompt_tokens: 5,
                request: handle1.clone(),
            },
        })
        .expect("send embedding");
        tx1.send(Event {
            id: req1.params.id.clone(),
            kind: EventKind::Finish {
                request: handle1,
                reason: StopReason::Eos,
            },
        })
        .expect("send finish");

        let resp = route_task.await.expect("task join");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = response_body_text(resp).await;
        let json: serde_json::Value = serde_json::from_str(&body).expect("json body");
        assert_eq!(json["object"], "list");
        assert_eq!(json["usage"]["prompt_tokens"], 12);
        assert_eq!(json["usage"]["total_tokens"], 12);
        assert_eq!(json["data"][0]["index"], 0);
        assert_eq!(json["data"][1]["index"], 1);
        assert_eq!(json["data"][0]["embedding"][0], 0.11);
        assert_eq!(json["data"][1]["embedding"][0], 0.21);
    }

    #[tokio::test]
    async fn embeddings_returns_base64_when_requested() {
        use base64::Engine as _;

        let (state, mut engine_rx) = make_embeddings_state_with_engine_rx();
        let request = EmbeddingRequest {
            model: "test".to_string(),
            input: EmbeddingInput::String("hello".to_string()),
            encoding_format: Some("base64".to_string()),
            dimensions: None,
            user: None,
        };

        let route_task = tokio::spawn(async move {
            embeddings(State(state), Json(request))
                .await
                .into_response()
        });

        let mut req = engine_rx.recv().await.expect("request");
        let tx = req.response_tx.take().expect("response channel");
        let handle = RequestHandle::new(req.params.id.clone(), req.cancel_flag.clone());
        tx.send(Event {
            id: req.params.id.clone(),
            kind: EventKind::Embedding {
                embedding: vec![1.0, -2.5],
                prompt_tokens: 2,
                request: handle.clone(),
            },
        })
        .expect("send embedding");
        tx.send(Event {
            id: req.params.id.clone(),
            kind: EventKind::Finish {
                request: handle,
                reason: StopReason::Eos,
            },
        })
        .expect("send finish");

        let resp = route_task.await.expect("task join");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = response_body_text(resp).await;
        let json: serde_json::Value = serde_json::from_str(&body).expect("json body");
        let encoded = json["data"][0]["embedding"]
            .as_str()
            .expect("base64 embedding");
        let raw = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("decode base64");
        assert_eq!(raw.len(), 8);
        assert_eq!(json["usage"]["prompt_tokens"], 2);
    }

    #[tokio::test]
    async fn embeddings_rejects_invalid_encoding_format() {
        let state = make_embeddings_state();
        let resp = embeddings(
            State(state),
            Json(EmbeddingRequest {
                model: "test".to_string(),
                input: EmbeddingInput::String("hello".to_string()),
                encoding_format: Some("binary".to_string()),
                dimensions: None,
                user: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = response_body_text(resp).await;
        let json: serde_json::Value = serde_json::from_str(&body).expect("json body");
        assert_eq!(json["error"]["type"], "invalid_request_error");
        assert_eq!(json["error"]["param"], "encoding_format");
    }

    #[tokio::test]
    async fn embeddings_rejects_when_embeddings_mode_disabled() {
        let state = make_test_state();
        let resp = embeddings(
            State(state),
            Json(EmbeddingRequest {
                model: "test".to_string(),
                input: EmbeddingInput::String("hello".to_string()),
                encoding_format: Some("float".to_string()),
                dimensions: None,
                user: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = response_body_text(resp).await;
        let json: serde_json::Value = serde_json::from_str(&body).expect("json body");
        assert_eq!(json["error"]["type"], "invalid_request_error");
        assert_eq!(json["error"]["param"], "mode");
    }

    #[tokio::test]
    async fn embeddings_accepts_token_array_input() {
        let (state, mut engine_rx) = make_embeddings_state_with_engine_rx();
        let request = EmbeddingRequest {
            model: "test".to_string(),
            input: EmbeddingInput::Tokens(vec![1, 2, 3]),
            encoding_format: Some("float".to_string()),
            dimensions: None,
            user: None,
        };

        let route_task = tokio::spawn(async move {
            embeddings(State(state), Json(request))
                .await
                .into_response()
        });

        let mut req = engine_rx.recv().await.expect("request");
        assert_eq!(
            req.params.prompt_tokens_override.as_ref().map(|v| v.len()),
            Some(3)
        );
        assert_eq!(req.params.prompt, "");

        let tx = req.response_tx.take().expect("response channel");
        let handle = RequestHandle::new(req.params.id.clone(), req.cancel_flag.clone());
        tx.send(Event {
            id: req.params.id.clone(),
            kind: EventKind::Embedding {
                embedding: vec![0.5, 0.7],
                prompt_tokens: 3,
                request: handle.clone(),
            },
        })
        .expect("send embedding");
        tx.send(Event {
            id: req.params.id.clone(),
            kind: EventKind::Finish {
                request: handle,
                reason: StopReason::Eos,
            },
        })
        .expect("send finish");

        let resp = route_task.await.expect("task join");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_text(resp).await;
        let json: serde_json::Value = serde_json::from_str(&body).expect("json body");
        assert_eq!(json["usage"]["prompt_tokens"], 3);
    }
}
