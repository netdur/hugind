use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub api_key: Option<String>,
    pub max_slots: u32,
    pub system_prompt: String,
    pub system_prompt_file: Option<PathBuf>,
    pub embeddings_enabled: bool,
    pub session_home: PathBuf,
    pub unified_memory_mode: bool,
    pub verbose: bool,
    pub model_path: PathBuf,
    pub mmproj_path: Option<PathBuf>,
    pub model_name: Option<String>,
    pub model_params: ModelParams,
    pub context_params: ContextParams,
    pub multimodal_params: MultimodalParams,
    pub sampler_params: SamplerParams,
    pub chat_params: ChatParams,
    pub lora_params: LoraParams,
    pub fit_params: FitParams,
    pub quantize_params: QuantizeParams,
    pub advanced_params: AdvancedParams,
    // Legacy compatibility: mirrors chat.format when present.
    pub chat_format: Option<ChatFormat>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ModelParams {
    pub devices: Vec<i32>,
    pub tensor_buft_overrides: Vec<String>,
    pub kv_overrides: Vec<String>,
    #[serde(alias = "n_gpu_layers", rename = "gpu_layers")]
    pub n_gpu_layers: i32,
    pub split_mode: SplitMode,
    pub main_gpu: i32,
    pub tensor_split: Vec<f32>,
    pub vocab_only: bool,
    pub use_mmap: bool,
    pub use_direct_io: bool,
    pub use_mlock: bool,
    pub check_tensors: bool,
    pub use_extra_bufts: bool,
    pub no_host: bool,
    pub no_alloc: bool,
}

impl Default for ModelParams {
    fn default() -> Self {
        Self {
            devices: Vec::new(),
            tensor_buft_overrides: Vec::new(),
            kv_overrides: Vec::new(),
            n_gpu_layers: 99,
            split_mode: SplitMode::Layer,
            main_gpu: 0,
            tensor_split: Vec::new(),
            vocab_only: false,
            use_mmap: true,
            use_direct_io: false,
            use_mlock: false,
            check_tensors: false,
            use_extra_bufts: true,
            no_host: false,
            no_alloc: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ContextParams {
    #[serde(alias = "n_ctx", rename = "size")]
    pub n_ctx: u32,
    #[serde(alias = "n_batch", rename = "batch_size")]
    pub n_batch: u32,
    #[serde(alias = "n_ubatch", rename = "ubatch_size")]
    pub n_ubatch: u32,
    #[serde(alias = "n_seq_max", rename = "seq_max")]
    pub n_seq_max: u32,
    #[serde(alias = "n_threads", rename = "threads")]
    pub n_threads: i32,
    #[serde(alias = "n_threads_batch", rename = "threads_batch")]
    pub n_threads_batch: i32,
    pub rope_scaling_type: RopeScalingType,
    pub pooling_type: PoolingType,
    pub attention_type: AttentionType,
    pub flash_attention: bool,
    pub flash_attn_type: FlashAttnType,
    pub rope_freq_base: f32,
    pub rope_freq_scale: f32,
    pub yarn_ext_factor: f32,
    pub yarn_attn_factor: f32,
    pub yarn_beta_fast: f32,
    pub yarn_beta_slow: f32,
    pub yarn_orig_ctx: u32,
    #[serde(rename = "cache_type_k")]
    pub type_k: CacheType,
    #[serde(rename = "cache_type_v")]
    pub type_v: CacheType,
    pub offload_kqv: bool,
    pub kv_unified: bool,
    pub swa_full: bool,
    pub op_offload: bool,
    pub embeddings: bool,
    pub no_perf: bool,
    pub defrag_thold: f32,
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
            rope_scaling_type: RopeScalingType::Unspecified,
            pooling_type: PoolingType::Unspecified,
            attention_type: AttentionType::Unspecified,
            flash_attention: false,
            flash_attn_type: FlashAttnType::Auto,
            rope_freq_base: 0.0,
            rope_freq_scale: 0.0,
            yarn_ext_factor: -1.0,
            yarn_attn_factor: 1.0,
            yarn_beta_fast: 32.0,
            yarn_beta_slow: 1.0,
            yarn_orig_ctx: 0,
            type_k: CacheType::F16,
            type_v: CacheType::F16,
            offload_kqv: true,
            kv_unified: true,
            swa_full: true,
            op_offload: true,
            embeddings: false,
            no_perf: false,
            defrag_thold: -1.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MultimodalParams {
    pub mmproj_offload: bool,
    pub image_min_tokens: i32,
    pub image_max_tokens: i32,
}

impl Default for MultimodalParams {
    fn default() -> Self {
        Self {
            mmproj_offload: true,
            image_min_tokens: 0,
            image_max_tokens: 0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SamplerParams {
    pub no_perf: bool,
    pub seed: u32,
    pub temp: f32,
    pub top_k: i32,
    pub top_p: f32,
    pub min_p: f32,
    pub typical_p: f32,
    pub top_n_sigma: f32,
    pub dynatemp_range: f32,
    pub dynatemp_exp: f32,
    pub repeat_last_n: i32,
    pub repeat_penalty: f32,
    pub frequency_penalty: f32,
    pub presence_penalty: f32,
    pub dry_multiplier: f32,
    pub dry_base: f32,
    pub dry_allowed_length: u32,
    pub dry_penalty_last_n: i32,
    pub dry_sequence_breakers: Vec<String>,
    pub xtc_probability: f32,
    pub xtc_threshold: f32,
    pub adaptive_target: f32,
    pub adaptive_decay: f32,
    pub mirostat: i32,
    pub mirostat_lr: f32,
    pub mirostat_ent: f32,
    pub logit_bias: Vec<LogitBiasItem>,
    pub grammar: String,
}

impl Default for SamplerParams {
    fn default() -> Self {
        Self {
            no_perf: false,
            seed: u32::MAX,
            temp: 0.7,
            top_k: 40,
            top_p: 0.95,
            min_p: 0.05,
            typical_p: 1.0,
            top_n_sigma: -1.0,
            dynatemp_range: 0.0,
            dynatemp_exp: 1.0,
            repeat_last_n: 64,
            repeat_penalty: 1.1,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
            dry_multiplier: 0.0,
            dry_base: 1.75,
            dry_allowed_length: 2,
            dry_penalty_last_n: -1,
            dry_sequence_breakers: vec![
                "\n".to_string(),
                ":".to_string(),
                "\"".to_string(),
                "*".to_string(),
            ],
            xtc_probability: 0.0,
            xtc_threshold: 0.1,
            adaptive_target: -1.0,
            adaptive_decay: 0.0,
            mirostat: 0,
            mirostat_lr: 0.1,
            mirostat_ent: 5.0,
            logit_bias: Vec::new(),
            grammar: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LogitBiasItem {
    pub token: i32,
    pub bias: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ChatParams {
    pub enable_thinking_default: bool,
    pub thinking_budget_tokens: Option<u32>,
    pub format: Option<ChatFormat>,
}

impl Default for ChatParams {
    fn default() -> Self {
        Self {
            enable_thinking_default: false,
            thinking_budget_tokens: None,
            format: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct LoraParams {
    pub adapters: Vec<PathBuf>,
    pub scaled_adapters: Vec<ScaledAdapter>,
    pub control_vectors: Vec<ControlVector>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScaledAdapter {
    pub path: PathBuf,
    #[serde(default = "default_scale")]
    pub scale: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ControlVector {
    pub path: PathBuf,
    #[serde(default = "default_scale")]
    pub scale: f32,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct FitParams {
    pub enabled: bool,
    pub target_mib: Vec<usize>,
    pub min_ctx: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct QuantizeParams {
    pub nthread: i32,
    pub ftype: String,
    pub output_tensor_type: String,
    pub token_embedding_type: String,
    pub allow_requantize: bool,
    pub quantize_output_tensor: bool,
    pub only_copy: bool,
    pub pure: bool,
    pub keep_split: bool,
    pub dry_run: bool,
}

impl Default for QuantizeParams {
    fn default() -> Self {
        Self {
            nthread: 0,
            ftype: "mostly_q4_0".to_string(),
            output_tensor_type: "f16".to_string(),
            token_embedding_type: "f16".to_string(),
            allow_requantize: false,
            quantize_output_tensor: true,
            only_copy: false,
            pure: false,
            keep_split: false,
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AdvancedParams {
    pub numa: String,
    pub warmup: bool,
}

impl Default for AdvancedParams {
    fn default() -> Self {
        Self {
            numa: String::new(),
            warmup: true,
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

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RopeScalingType {
    #[default]
    Unspecified,
    None,
    Linear,
    Yarn,
    Longrope,
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PoolingType {
    #[default]
    Unspecified,
    None,
    Mean,
    Cls,
    Last,
    Rank,
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AttentionType {
    #[default]
    Unspecified,
    Causal,
    #[serde(alias = "non-causal")]
    NonCausal,
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FlashAttnType {
    #[default]
    Auto,
    On,
    Off,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatFormat {
    Llama2,
    Chatml,
    Gemma,
    Vicuna,
    #[serde(alias = "zepyhr")]
    Zephyr,
    Openchat,
    Deepseek,
    Qwen3,
}

fn default_scale() -> f32 {
    1.0
}
