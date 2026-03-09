use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatCompletionMessage>,
    pub stream: Option<bool>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub frequency_penalty: Option<f32>,
    pub presence_penalty: Option<f32>,
    pub stop: Option<Stop>,
    pub response_format: Option<ResponseFormat>,
    #[serde(alias = "thinking")]
    pub enable_thinking: Option<bool>,
    #[serde(alias = "thinking_budget")]
    pub thinking_budget_tokens: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum Stop {
    String(String),
    Array(Vec<String>),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChatCompletionMessage {
    pub role: String,
    pub content: Content,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Multimodal(Vec<MultimodalContent>),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type")]
pub enum MultimodalContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl },
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ImageUrl {
    pub url: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ResponseFormat {
    #[serde(rename = "type")]
    pub format_type: String,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatCompletionChoice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionChoice {
    pub index: u32,
    pub message: ChatCompletionChoiceMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionChoiceMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatCompletionChunkChoice>,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionChunkChoice {
    pub index: u32,
    pub delta: ChatCompletionChunkDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionChunkDelta {
    pub role: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ModelList {
    pub object: String,
    pub data: Vec<ModelInfo>,
}

#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
}

#[derive(Debug, Serialize)]
pub struct MonitorStats {
    pub config_name: String,
    pub server_state: String,
    pub requests_processing: usize,
    pub requests_waiting: usize,
    pub tokens_per_sec_total: f64,
    pub tokens_per_sec_per_active: f64,
    pub slots_usage: SlotsUsage,
    pub memory: MemoryStats,
    pub cache_stats: CacheStats,
}

#[derive(Debug, Serialize)]
pub struct SlotsUsage {
    pub active: usize,
    pub total: usize,
}

#[derive(Debug, Serialize)]
pub struct MemoryStats {
    pub ram_usage_bytes: u64,
    pub vram_usage_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct CacheStats {
    pub vram_sessions: usize,
    pub ram_sessions: usize,
}

#[derive(Debug, Deserialize)]
pub struct StateSaveRequest {
    pub session_id: String,
    pub template_id: String,
}

#[derive(Debug, Deserialize)]
pub struct StateIdleRequest {
    pub session_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StateStatusResponse {
    pub session_id: String,
    pub exists: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct EmbeddingRequest {
    pub model: String,
    pub input: EmbeddingInput,
    pub encoding_format: Option<String>,
    pub dimensions: Option<u32>,
    pub user: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum EmbeddingInput {
    String(String),
    StringArray(Vec<String>),
    Tokens(Vec<i64>),
    TokenArray(Vec<Vec<i64>>),
}

#[derive(Debug, Serialize)]
pub struct EmbeddingResponse {
    pub object: String,
    pub data: Vec<EmbeddingData>,
    pub model: String,
    pub usage: EmbeddingUsage,
}

#[derive(Debug, Serialize)]
pub struct EmbeddingData {
    pub object: String,
    pub index: usize,
    pub embedding: EmbeddingVector,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum EmbeddingVector {
    Float(Vec<f32>),
    Base64(String),
}

#[derive(Debug, Serialize)]
pub struct EmbeddingUsage {
    pub prompt_tokens: u32,
    pub total_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::{ChatCompletionRequest, Content, MultimodalContent};

    #[test]
    fn deserializes_thinking_aliases() {
        let raw = r#"
        {
          "model": "test-model",
          "messages": [{"role":"user","content":"hello"}],
          "thinking": true,
          "thinking_budget": 128
        }
        "#;

        let req: ChatCompletionRequest = serde_json::from_str(raw).expect("valid json");
        assert_eq!(req.enable_thinking, Some(true));
        assert_eq!(req.thinking_budget_tokens, Some(128));
    }

    #[test]
    fn deserializes_multimodal_message_content() {
        let raw = r#"
        {
          "model": "test-model",
          "messages": [
            {
              "role": "user",
              "content": [
                {"type":"text","text":"look"},
                {"type":"image_url","image_url":{"url":"data:image/png;base64,aGVsbG8="}}
              ]
            }
          ]
        }
        "#;

        let req: ChatCompletionRequest = serde_json::from_str(raw).expect("valid json");
        assert_eq!(req.messages.len(), 1);
        match &req.messages[0].content {
            Content::Multimodal(parts) => {
                assert_eq!(parts.len(), 2);
                assert!(matches!(parts[0], MultimodalContent::Text { .. }));
                assert!(matches!(parts[1], MultimodalContent::ImageUrl { .. }));
            }
            _ => panic!("expected multimodal content"),
        }
    }
}
