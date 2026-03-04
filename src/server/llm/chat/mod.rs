use crate::llm::error::{Error, Result};
use crate::llm::model::Model;
use llama_cpp::{ChatMessage, ChatTemplateOptions, apply_chat_template_with_kwargs, llama_model};
use serde_json::Map;

pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn new(role: &str, content: &str) -> Self {
        Self {
            role: role.to_string(),
            content: content.to_string(),
        }
    }
}

pub fn template(model: &Model, prompt: &str) -> Result<String> {
    let messages = vec![Message::new("user", prompt)];
    apply_template(model, &messages, None, false)
}

pub fn apply_template(
    model: &Model,
    messages: &[Message],
    enable_thinking: Option<bool>,
    enable_thinking_default: bool,
) -> Result<String> {
    let chat_messages: Vec<ChatMessage<'_>> = messages
        .iter()
        .map(|m| ChatMessage {
            role: m.role.as_str(),
            content: m.content.as_str(),
        })
        .collect();

    let options = ChatTemplateOptions {
        add_generation_prompt: true,
        enable_thinking_default,
        enable_thinking,
        chat_template_kwargs: Map::new(),
    };

    apply_chat_template_with_kwargs(
        model.as_ptr() as *const llama_model,
        None,
        &chat_messages,
        &options,
    )
    .map_err(|e| Error::BackendError(format!("Failed to apply chat template: {:?}", e)))
}
