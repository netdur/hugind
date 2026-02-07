use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::{LlamaChatMessage};

pub fn apply_chat_template(
    model: &LlamaModel, 
    user_prompt: &str
) -> Result<String, Box<dyn std::error::Error>> {
    if let Ok(template) = model.chat_template(None) {
        let messages = vec![
            LlamaChatMessage::new("user".to_string(), user_prompt.to_string())?
        ];
        
        match model.apply_chat_template(&template, &messages, true) {
            Ok(formatted) => Ok(formatted),
            Err(e) => {
                eprintln!("Failed to apply chat template: {}. Using raw prompt.", e);
                Ok(user_prompt.to_string())
            }
        }
    } else {
        Ok(user_prompt.to_string())
    }
}
