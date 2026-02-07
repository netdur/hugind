use rquickjs::{AsyncContext, Class, Result};
use rquickjs::class::Trace; 
use crate::core::config::agent::AgentConfig;
use crate::core::config::backend::resolve_backend;


#[rquickjs::class]
pub struct Llm {
    client: reqwest::Client,
    base_url: String,
    model: Option<String>,
}

impl<'js> Trace<'js> for Llm {
    fn trace<'a>(&self, _tracer: rquickjs::class::Tracer<'a, 'js>) {
        
    }
}



impl Llm {
    pub fn new(config: &AgentConfig) -> Self {
        let resolved = resolve_backend(config)
            .map_err(|e| rquickjs::Error::new_loading_message("Backend Config Error", e.to_string()))
            .unwrap_or_else(|_| crate::core::config::backend::ResolvedBackend {
                base_url: "http://127.0.0.1:8080/v1".to_string(),
                health_url: "http://127.0.0.1:8080/v1/monitor".to_string(),
                model: Some("default".to_string()),
            });

        Self {
            client: reqwest::Client::new(),
            base_url: resolved.base_url,
            model: resolved.model,
        }
    }
}

#[rquickjs::methods]
impl Llm {
    pub async fn chat(&self, prompt: String) -> Result<String> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        
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

        let res = self.client.post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| rquickjs::Error::new_loading_message("LLM Request Failed", e.to_string()))?;

        if !res.status().is_success() {
             let status = res.status();
             let text = res.text().await.unwrap_or_default();
             return Err(rquickjs::Error::new_loading_message("LLM Error", format!("{}: {}", status, text)));
        }

        let body_text = res.text().await
             .map_err(|e| rquickjs::Error::new_loading_message("Response Read Error", e.to_string()))?;

        
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&body_text) {
             if let Some(content) = data.get("choices")
                .and_then(|choices| choices.get(0))
                .and_then(|choice| choice.get("message"))
                .and_then(|message| message.get("content"))
                .and_then(|content| content.as_str()) 
            {
                return Ok(content.to_string());
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

        
        if let Some(json_str) = extract_markdown_json(&body_text) {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&json_str) {
                 if let Some(content) = data.get("choices")
                    .and_then(|choices| choices.get(0))
                    .and_then(|choice| choice.get("message"))
                    .and_then(|message| message.get("content"))
                    .and_then(|content| content.as_str()) 
                {
                    return Ok(content.to_string());
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
                         if let Some(delta) = data.get("choices")
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
                return Ok(full_content);
            }
        }

        
        Ok(body_text)
    }
}

pub async fn install(ctx: &AsyncContext, config: &AgentConfig) -> Result<()> {
    let llm = Llm::new(config);
    
    ctx.async_with(|ctx| Box::pin(async move {
        let llm_cls = Class::instance(ctx.clone(), llm)?;
        ctx.globals().set("llm", llm_cls)?;
        Ok(())
    })).await
}
