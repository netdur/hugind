use crate::llm::tokenizer::Token;

#[derive(Debug, Clone)]
pub struct SessionState {
    pub pos: i32,
    pub seq_id: i32,
    pub history: Vec<Token>,
}

impl SessionState {
    pub fn new(seq_id: i32) -> Self {
        Self {
            pos: 0,
            seq_id,
            history: Vec::new(),
        }
    }
}
