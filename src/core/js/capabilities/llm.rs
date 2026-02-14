use futures::StreamExt;
use rquickjs::{AsyncContext, Class, Result};
use rquickjs::class::Trace; 
use crate::core::config::agent::AgentConfig;
use crate::core::config::backend::resolve_backend;
use crate::shared::logging::RunLogger;


#[derive(rquickjs::JsLifetime)]
#[rquickjs::class]
pub struct Llm {
    client: reqwest::Client,
    base_url: String,
    model: Option<String>,
    session_id: Option<String>,
    logger: Option<RunLogger>,
}

impl<'js> Trace<'js> for Llm {
    fn trace<'a>(&self, _tracer: rquickjs::class::Tracer<'a, 'js>) {
        
    }
}



impl Llm {
    pub fn new(config: &AgentConfig, logger: Option<RunLogger>) -> Self {
        let resolved = resolve_backend(config)
            .map_err(|e| rquickjs::Error::new_loading_message("Backend Config Error", e.to_string()))
            .unwrap_or_else(|_| crate::core::config::backend::ResolvedBackend {
                base_url: "http://127.0.0.1:8080/v1".to_string(),
                health_url: "http://127.0.0.1:8080/v1/monitor".to_string(),
                model: Some("default".to_string()),
                session: None,
            });

        Self {
            client: reqwest::Client::new(),
            base_url: resolved.base_url,
            model: resolved.model,
            session_id: resolved.session.and_then(|s| s.id),
            logger,
        }
    }
}

#[rquickjs::methods]
impl Llm {
    pub async fn chat(&self, input: rquickjs::Value<'_>) -> Result<String> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let mut logged = false;
        let mut body = match js_value_to_json(input)? {
            serde_json::Value::String(prompt) => {
                if let Some(logger) = &self.logger {
                    logger.log_line(format!(
                        "host.llm.chat input=string prompt_len={}",
                        prompt.len()
                    ));
                    logged = true;
                }
                let mut messages = Vec::new();
                messages.push(serde_json::json!({
                    "role": "user",
                    "content": prompt
                }));

                let mut body = serde_json::Map::new();
                if let Some(m) = &self.model {
                    body.insert("model".to_string(), serde_json::json!(m));
                }
                body.insert("messages".to_string(), serde_json::json!(messages));
                body.insert("stream".to_string(), serde_json::json!(false));
                body.insert(
                    "response_format".to_string(),
                    serde_json::json!({ "type": "json_object" }),
                );
                body
            }
            serde_json::Value::Object(map) => map,
            _ => {
                return Err(rquickjs::Error::new_loading_message(
                    "LLM Error",
                    "chat() expects a string prompt or an object request body.",
                ));
            }
        };
        if !logged {
            if let Some(logger) = &self.logger {
                let msg_len = body
                    .get("messages")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len());
                let model = body.get("model").and_then(|v| v.as_str()).unwrap_or("");
                logger.log_line(format!(
                    "host.llm.chat input=object messages={:?} model={}",
                    msg_len, model
                ));
            }
        }

        if !body.contains_key("messages") {
            if let Some(prompt_val) = body.remove("prompt") {
                if let Some(prompt) = prompt_val.as_str() {
                    let messages = vec![serde_json::json!({
                        "role": "user",
                        "content": prompt
                    })];
                    body.insert("messages".to_string(), serde_json::json!(messages));
                } else {
                    return Err(rquickjs::Error::new_loading_message(
                        "LLM Error",
                        "chat() prompt must be a string.",
                    ));
                }
            } else {
                return Err(rquickjs::Error::new_loading_message(
                    "LLM Error",
                    "chat() request body must include messages or prompt.",
                ));
            }
        }

        if !body.contains_key("model") {
            if let Some(m) = &self.model {
                body.insert("model".to_string(), serde_json::json!(m));
            }
        }

        if !body.contains_key("stream") {
            body.insert("stream".to_string(), serde_json::json!(false));
        }
        if !body.contains_key("response_format") {
            body.insert(
                "response_format".to_string(),
                serde_json::json!({ "type": "json_object" }),
            );
        }

        let mut request = self
            .client
            .post(&url)
            .timeout(std::time::Duration::from_secs(120))
            .json(&body);
        if let Some(id) = &self.session_id {
            request = request.header("X-Session-ID", id);
        }
        let res = request
            .send()
            .await
            .map_err(|e| rquickjs::Error::new_loading_message("LLM Request Failed", e.to_string()))?;

        if !res.status().is_success() {
             let status = res.status();
             let text = res.text().await.unwrap_or_default();
             return Err(rquickjs::Error::new_loading_message("LLM Error", format!("{}: {}", status, text)));
        }

        let body_text = read_response_limited(res).await?;
        let content = extract_llm_content(&body_text);
        if let Some(logger) = &self.logger {
            logger.log_line(format!("host.llm.chat response_len={}", content.len()));
        }
        Ok(content)
    }

    pub async fn chat_stream(&self, input: rquickjs::Value<'_>) -> Result<String> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let mut on_token: Option<rquickjs::Function> = None;
        if input.is_object() {
            let obj = input.clone().into_object().unwrap();
            if let Ok(cb) = obj.get::<_, rquickjs::Function>("on_token") {
                on_token = Some(cb);
            } else if let Ok(cb) = obj.get::<_, rquickjs::Function>("onToken") {
                on_token = Some(cb);
            }
        }

        let mut logged = false;
        let mut body = match js_value_to_json(input)? {
            serde_json::Value::String(prompt) => {
                if let Some(logger) = &self.logger {
                    logger.log_line(format!(
                        "host.llm.chat_stream input=string prompt_len={}",
                        prompt.len()
                    ));
                    logged = true;
                }
                let messages = vec![serde_json::json!({
                    "role": "user",
                    "content": prompt
                })];

                let mut body = serde_json::Map::new();
                if let Some(m) = &self.model {
                    body.insert("model".to_string(), serde_json::json!(m));
                }
                body.insert("messages".to_string(), serde_json::json!(messages));
                body.insert("stream".to_string(), serde_json::json!(true));
                body.insert(
                    "response_format".to_string(),
                    serde_json::json!({ "type": "json_object" }),
                );
                body
            }
            serde_json::Value::Object(mut map) => {
                map.remove("on_token");
                map.remove("onToken");
                map
            }
            _ => {
                return Err(rquickjs::Error::new_loading_message(
                    "LLM Error",
                    "chat_stream() expects a string prompt or an object request body.",
                ));
            }
        };
        if !logged {
            if let Some(logger) = &self.logger {
                let msg_len = body
                    .get("messages")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len());
                let model = body.get("model").and_then(|v| v.as_str()).unwrap_or("");
                logger.log_line(format!(
                    "host.llm.chat_stream input=object messages={:?} model={}",
                    msg_len, model
                ));
            }
        }

        if !body.contains_key("messages") {
            if let Some(prompt_val) = body.remove("prompt") {
                if let Some(prompt) = prompt_val.as_str() {
                    let messages = vec![serde_json::json!({
                        "role": "user",
                        "content": prompt
                    })];
                    body.insert("messages".to_string(), serde_json::json!(messages));
                } else {
                    return Err(rquickjs::Error::new_loading_message(
                        "LLM Error",
                        "chat_stream() prompt must be a string.",
                    ));
                }
            } else {
                return Err(rquickjs::Error::new_loading_message(
                    "LLM Error",
                    "chat_stream() request body must include messages or prompt.",
                ));
            }
        }

        if !body.contains_key("model") {
            if let Some(m) = &self.model {
                body.insert("model".to_string(), serde_json::json!(m));
            }
        }

        if !body.contains_key("stream") {
            body.insert("stream".to_string(), serde_json::json!(true));
        }
        if !body.contains_key("response_format") {
            body.insert(
                "response_format".to_string(),
                serde_json::json!({ "type": "json_object" }),
            );
        }

        let mut request = self
            .client
            .post(&url)
            .timeout(std::time::Duration::from_secs(120))
            .json(&body);
        if let Some(id) = &self.session_id {
            request = request.header("X-Session-ID", id);
        }
        let res = request
            .send()
            .await
            .map_err(|e| rquickjs::Error::new_loading_message("LLM Request Failed", e.to_string()))?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(rquickjs::Error::new_loading_message(
                "LLM Error",
                format!("{}: {}", status, text),
            ));
        }

        let mut content = String::new();
        let mut stream = res.bytes_stream();

        while let Some(item) = stream.next().await {
            let chunk = item
                .map_err(|e| rquickjs::Error::new_loading_message("LLM Error", e.to_string()))?;
            let text = String::from_utf8_lossy(&chunk);
            for line in text.lines() {
                if !line.starts_with("data: ") {
                    continue;
                }
                let data_str = &line[6..];
                if data_str.trim() == "[DONE]" {
                    continue;
                }
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(data_str) {
                    if let Some(delta) = data
                        .get("choices")
                        .and_then(|choices| choices.get(0))
                        .and_then(|choice| choice.get("delta"))
                        .and_then(|delta| delta.get("content"))
                        .and_then(|content| content.as_str())
                    {
                        content.push_str(delta);
                        if let Some(cb) = &on_token {
                            let _ = cb.call::<_, ()>((delta,));
                        }
                    }
                }
            }
        }

        if let Some(logger) = &self.logger {
            logger.log_line(format!(
                "host.llm.chat_stream response_len={}",
                content.len()
            ));
        }
        Ok(content)
    }
}

fn js_value_to_json<'js>(value: rquickjs::Value<'js>) -> rquickjs::Result<serde_json::Value> {
    if value.is_null() || value.is_undefined() {
        Ok(serde_json::Value::Null)
    } else if value.is_bool() {
        Ok(serde_json::Value::Bool(value.as_bool().unwrap()))
    } else if value.is_number() {
        let n = value.as_number().unwrap();
        if n.is_finite() && (n.fract() == 0.0) {
            if n >= (i64::MIN as f64) && n <= (i64::MAX as f64) {
                return Ok(serde_json::Value::Number(serde_json::Number::from(n as i64)));
            }
        }
        Ok(serde_json::Value::Number(
            serde_json::Number::from_f64(n).unwrap_or_else(|| serde_json::Number::from(0)),
        ))
    } else if value.is_string() {
        let s: rquickjs::String = value.into_string().unwrap();
        Ok(serde_json::Value::String(s.to_string()?))
    } else if value.is_array() {
        let arr = value.into_array().unwrap();
        let mut out = Vec::new();
        for i in 0..arr.len() {
            out.push(js_value_to_json(arr.get(i)?)?);
        }
        Ok(serde_json::Value::Array(out))
    } else if value.is_object() {
        let obj = value.into_object().unwrap();
        let mut out = serde_json::Map::new();
        for key in obj.keys::<rquickjs::String>() {
            let key = key?;
            let k_str = key.to_string()?;
            let v = obj.get(&k_str)?;
            out.insert(k_str, js_value_to_json(v)?);
        }
        Ok(serde_json::Value::Object(out))
    } else {
        Ok(serde_json::Value::Null)
    }
}

pub async fn install(ctx: &AsyncContext, config: &AgentConfig, logger: Option<RunLogger>) -> Result<()> {
    let llm = Llm::new(config, logger);
    
    ctx.async_with(|ctx| Box::pin(async move {
        let llm_cls = Class::instance(ctx.clone(), llm)?;
        ctx.globals().set("llm", llm_cls)?;
        Ok(())
    })).await
}

async fn read_response_limited(res: reqwest::Response) -> Result<String> {
    let mut content = Vec::new();
    let mut stream = res.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item
            .map_err(|e| rquickjs::Error::new_loading_message("Response Read Error", e.to_string()))?;
        content.extend_from_slice(&chunk);
    }

    Ok(String::from_utf8_lossy(&content).to_string())
}

fn extract_llm_content(body_text: &str) -> String {
    if let Ok(data) = serde_json::from_str::<serde_json::Value>(body_text) {
        if let Some(content) = data
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(|content| content.as_str())
        {
            return content.to_string();
        }
    }

    let extract_markdown_json = |text: &str| -> Option<String> {
        let start_marker = "```json";
        let end_marker = "```";
        if let Some(start_idx) = text.find(start_marker) {
            let content_start = start_idx + start_marker.len();
            if let Some(end_idx) = text[content_start..].find(end_marker) {
                return Some(text[content_start..content_start + end_idx].to_string());
            }
        }
        None
    };

    if let Some(json_str) = extract_markdown_json(body_text) {
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&json_str) {
            if let Some(content) = data
                .get("choices")
                .and_then(|choices| choices.get(0))
                .and_then(|choice| choice.get("message"))
                .and_then(|message| message.get("content"))
                .and_then(|content| content.as_str())
            {
                return content.to_string();
            }
        }
    }

    if body_text.starts_with("data: ") {
        let mut full_content = String::new();
        for line in body_text.lines() {
            if line.starts_with("data: ") {
                let data_str = &line[6..];
                if data_str.trim() == "[DONE]" {
                    continue;
                }
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(data_str) {
                    if let Some(delta) = data
                        .get("choices")
                        .and_then(|choices| choices.get(0))
                        .and_then(|choice| choice.get("delta"))
                        .and_then(|delta| delta.get("content"))
                        .and_then(|content| content.as_str())
                    {
                        full_content.push_str(delta);
                    }
                }
            }
        }
        if !full_content.is_empty() {
            return full_content;
        }
    }

    body_text.to_string()
}
