use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub name: String,
    
    #[serde(default = "default_host")]
    pub host: String,
    
    #[serde(default = "default_port")]
    pub port: u16,
    
    pub library_path: Option<PathBuf>,
    pub api_key: Option<String>,
    
    #[serde(default = "default_concurrency")]
    pub concurrency: u32,
    
    #[serde(default = "default_max_slots")]
    pub max_slots: u32,
    
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    #[serde(default)]
    pub system_prompt_file: Option<PathBuf>,
    
    #[serde(default)]
    pub embeddings_enabled: bool,
    
    #[serde(default = "default_session_home")]
    pub session_home: PathBuf,
    
    #[serde(default)]
    pub verbose: bool,

    
    pub model_path: PathBuf,
    pub mmproj_path: Option<PathBuf>,
    #[serde(default)]
    pub model_name: Option<String>,

    
    #[serde(default)]
    pub model_params: ModelParams,
    
    #[serde(default)]
    pub context_params: ContextParams,
    
    #[serde(default)]
    pub sampler_params: SamplerParams,
    
    pub chat_format: Option<ChatFormat>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelParams {
    #[serde(default = "default_gpu_layers")]
    pub n_gpu_layers: u32,
    
    #[serde(default)]
    pub split_mode: SplitMode,
    
    #[serde(default)]
    pub main_gpu: u32,
    
    #[serde(default = "default_true")]
    pub use_mmap: bool,
    
    #[serde(default)]
    pub use_mlock: bool,
    
    #[serde(default)]
    pub vocab_only: bool,
}

impl Default for ModelParams {
    fn default() -> Self {
        Self {
            n_gpu_layers: 99,
            split_mode: SplitMode::Layer,
            main_gpu: 0,
            use_mmap: true,
            use_mlock: false,
            vocab_only: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContextParams {
    #[serde(default = "default_n_ctx")]
    pub n_ctx: u32,
    
    #[serde(default = "default_batch_size")]
    pub n_batch: u32,
    
    #[serde(default = "default_ubatch_size")]
    pub n_ubatch: u32,
    
    
    #[serde(default = "default_max_slots")] 
    pub n_seq_max: u32,
    
    #[serde(default = "default_threads")]
    pub n_threads: u32,
    
    #[serde(default = "default_threads_batch")]
    pub n_threads_batch: u32,
    
    #[serde(default)]
    pub flash_attention: bool,
    
    #[serde(default)]
    pub type_k: CacheType,
    
    #[serde(default)]
    pub type_v: CacheType,
    
    #[serde(default = "default_true")]
    pub offload_kqv: bool,
    
    #[serde(default)]
    pub embeddings: bool,
}

impl Default for ContextParams {
    fn default() -> Self {
        Self {
            n_ctx: 4096,
            n_batch: 2048,
            n_ubatch: 512,
            n_seq_max: 4,
            n_threads: 8,
            n_threads_batch: 8,
            flash_attention: false,
            type_k: CacheType::F16,
            type_v: CacheType::F16,
            offload_kqv: true,
            embeddings: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SamplerParams {
    #[serde(default = "default_temp")]
    pub temp: f32,
    
    #[serde(default = "default_top_k")]
    pub top_k: u32,
    
    #[serde(default = "default_top_p")]
    pub top_p: f32,
    
    #[serde(default = "default_min_p")]
    pub min_p: f32,
    
    #[serde(default)]
    pub dry_multiplier: f32,
    #[serde(default = "default_repeat_last_n")]
    pub repeat_last_n: u32,
    #[serde(default = "default_repeat_penalty")]
    pub repeat_penalty: f32,
    #[serde(default = "default_frequency_penalty")]
    pub frequency_penalty: f32,
    #[serde(default = "default_presence_penalty")]
    pub presence_penalty: f32,
    #[serde(default = "default_dry_base")]
    pub dry_base: f32,
    #[serde(default = "default_dry_allowed_length")]
    pub dry_allowed_length: u32,
    #[serde(default = "default_xtc_probability")]
    pub xtc_probability: f32,
    #[serde(default = "default_xtc_threshold")]
    pub xtc_threshold: f32,
}

impl Default for SamplerParams {
    fn default() -> Self {
        Self {
            temp: 0.7,
            top_k: 40,
            top_p: 0.95,
            min_p: 0.05,
            dry_multiplier: 0.0,
            repeat_last_n: 64,
            repeat_penalty: 1.1,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
            dry_base: 1.75,
            dry_allowed_length: 2,
            xtc_probability: 0.0,
            xtc_threshold: 0.1,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SplitMode {
    None,
    #[default]
    Layer,
    Row,
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CacheType {
    F32,
    #[default]
    F16,
    Q4_0,
    Q4_1,
    Q5_0,
    Q5_1,
    Q8_0,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatFormat {
    Llama2,
    Chatml,
    Gemma,
    Vicuna,
    Zepyhr,
    Openchat,
    Deepseek,
    Qwen3,
    
}


fn default_host() -> String { "0.0.0.0".to_string() }
fn default_port() -> u16 { 8080 }
fn default_concurrency() -> u32 { 1 }
fn default_max_slots() -> u32 { 4 }
fn default_timeout() -> u64 { 600 }
fn default_system_prompt() -> String { "You are a helpful assistant.".to_string() }
fn default_session_home() -> PathBuf { "sessions".into() } 
fn default_gpu_layers() -> u32 { 99 }
fn default_true() -> bool { true }
fn default_n_ctx() -> u32 { 4096 }
fn default_batch_size() -> u32 { 2048 }
fn default_ubatch_size() -> u32 { 512 }
fn default_threads() -> u32 { 8 }
fn default_threads_batch() -> u32 { 8 }
fn default_temp() -> f32 { 0.7 }
fn default_top_k() -> u32 { 40 }
fn default_top_p() -> f32 { 0.95 }
fn default_min_p() -> f32 { 0.05 }
fn default_repeat_last_n() -> u32 { 64 }
fn default_repeat_penalty() -> f32 { 1.1 }
fn default_frequency_penalty() -> f32 { 0.0 }
fn default_presence_penalty() -> f32 { 0.0 }
fn default_dry_base() -> f32 { 1.75 }
fn default_dry_allowed_length() -> u32 { 2 }
fn default_xtc_probability() -> f32 { 0.0 }
fn default_xtc_threshold() -> f32 { 0.1 }
