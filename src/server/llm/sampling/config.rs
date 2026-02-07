#[derive(Debug, Clone)]
pub struct SamplingConfig {
    pub temp: f32,
    pub top_k: i32,
    pub top_p: f32,
    pub min_p: f32,
    pub penalty_repeat: f32,
    pub penalty_last_n: i32,
    pub greedy: bool,
    pub grammar: Option<GrammarParams>,
}

#[derive(Debug, Clone)]
pub struct GrammarParams {
    pub grammar: String,
    pub root: String,
}

impl Default for SamplingConfig {
    fn default() -> Self {
        Self {
            temp: 0.80,
            top_k: 40,
            top_p: 0.95,
            min_p: 0.05,
            penalty_repeat: 1.1,
            penalty_last_n: 64,
            greedy: false,
            grammar: None,
        }
    }
}
