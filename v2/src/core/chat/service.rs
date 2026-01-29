use anyhow::{Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::fs;
use crate::core::chat::session::Message;
use crate::shared::paths;

pub struct ChatService {
    default_base_url: String,
}

impl ChatService {
    pub fn new() -> Self {
        Self {
            default_base_url: "http://localhost:8080/v1/chat".to_string(),
        }
    }

    pub fn resolve_base_url(&self, config_name: &str) -> String {
        if config_name.is_empty() {
            return self.default_base_url.clone();
        }
        
        // Try direct config lookup
        // Assuming config_name maps to a file in <config_home>/configs/
        let config_path = paths::config_home().join("configs").join(format!("{}.yml", config_name));
        
        if config_path.exists() {
             if let Ok(content) = fs::read_to_string(&config_path) {
                 if let Ok(yaml) = serde_yaml::from_str::<Value>(&content) {
                     if let Some(server) = yaml.get("server") {
                         let host = server.get("host").and_then(|h| h.as_str()).unwrap_or("127.0.0.1");
                         let port = server.get("port").and_then(|p| p.as_u64()).unwrap_or(8080);
                         return format!("http://{}:{}/v1/chat", host, port);
                     }
                 }
             }
        }

        self.default_base_url.clone()
    }

    pub async fn send_message(
        &self,
        session_id: &str,
        model: &str,
        full_history: &[Message],
        new_message: &Message,
        is_new_session: bool,
        base_url: &str
    ) -> Result<reqwest::Response> {
        let client = Client::new();
        let url = format!("{}/completions", base_url);

        // 1. Optimistic Attempt
        let payload = json!({
            "model": model,
            "messages": [new_message],
            "stream": true
        });

        let response = client.post(&url)
            .header("Content-Type", "application/json")
            .header("X-Session-ID", session_id)
            .header("X-Fresh-Session", is_new_session.to_string())
            .json(&payload)
            .send()
            .await?;

        // 2. Fallback on 409
        if response.status() == 409 {
            // Rehydrate
             let mut messages_json = Vec::new();
             for msg in full_history {
                 messages_json.push(json!({
                     "role": msg.role,
                     "content": msg.content
                 }));
             }
             // Add new message
             messages_json.push(json!({
                 "role": new_message.role,
                 "content": new_message.content
             }));

             let rehydrate_payload = json!({
                "model": model,
                "messages": messages_json,
                "stream": true
            });

             let retry_response = client.post(&url)
                .header("Content-Type", "application/json")
                .header("X-Session-ID", session_id)
                .header("X-Fresh-Session", is_new_session.to_string())
                .json(&rehydrate_payload)
                .send()
                .await?;
            
            return Ok(retry_response);
        }

        Ok(response)
    }

    pub async fn generate_title(
        &self,
        model: &str,
        history: &[Message],
        base_url: &str
    ) -> String {
        let client = Client::new();
        let url = format!("{}/completions", base_url);

        let context = history.iter().take(6).map(|m| {
             json!({ "role": m.role, "content": m.content })
        }).collect::<Vec<_>>();

        let mut messages = context;
        messages.push(json!({
            "role": "user",
            "content": "Generate a short, concise 3-5 word title for this conversation. Do not use quotes."
        }));

         let payload = json!({
            "model": model,
            "messages": messages,
            "stream": false // No stream for simple title
        });

        if let Ok(resp) = client.post(&url).json(&payload).send().await {
            if let Ok(json) = resp.json::<Value>().await {
                 if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
                     if let Some(first) = choices.first() {
                         if let Some(content) = first.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_str()) {
                             return content.trim().replace('"', "").to_string();
                         }
                     }
                 }
            }
        }
        "".to_string()
    }

    pub async fn hibernate(&self, id: &str, base_url: &str) {
        let client = Client::new();
        let url = format!("{}/hibernate", base_url);
        let _ = client.post(&url)
            .header("X-Session-ID", id)
            .timeout(std::time::Duration::from_secs(1))
            .send().await;
    }
}
