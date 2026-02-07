use crate::llm::sampling::SamplingConfig;

#[derive(Debug, Clone, Default)]
pub struct GenerationConfig {
    pub max_tokens: Option<usize>,
    pub sampling: SamplingConfig,
}
