use std::num::NonZeroU32;
use llama_cpp_2::context::params::{LlamaContextParams, KvCacheType, RopeScalingType, LlamaPoolingType};
use llama_cpp_2::model::params::{LlamaModelParams, LlamaSplitMode};

/// Configuration for the LlamaContext.
/// Defaults match llama.cpp defaults where possible.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ContextParams {
    pub n_ctx: u32,
    pub n_batch: u32,
    pub n_ubatch: u32,
    pub n_seq_max: u32,
    pub n_threads: i32,
    pub n_threads_batch: i32,

    pub embeddings: bool,
    pub offload_kqv: bool,
    pub swa_full: bool,

    pub type_k: KvCacheType,
    pub type_v: KvCacheType,

    pub rope_scaling_type: RopeScalingType,
    pub rope_freq_base: f32,
    pub rope_freq_scale: f32,

    pub pooling_type: LlamaPoolingType,
    pub flash_attention: bool,

    pub n_predict: i32,
}

impl Default for ContextParams {
    fn default() -> Self {
        Self {
            n_ctx: 4096,
            n_batch: 512,
            n_ubatch: 512,
            n_seq_max: 4,
            n_threads: 8,
            n_threads_batch: 8,
            embeddings: false,
            offload_kqv: true,
            swa_full: false,
            type_k: KvCacheType::F16,
            type_v: KvCacheType::F16,
            rope_scaling_type: RopeScalingType::Unspecified,
            rope_freq_base: 0.0,
            rope_freq_scale: 0.0,
            pooling_type: LlamaPoolingType::Unspecified,
            flash_attention: false,
            n_predict: -1,
        }
    }
}

impl From<&ContextParams> for LlamaContextParams {
    fn from(params: &ContextParams) -> Self {
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(params.n_ctx))
            .with_n_batch(params.n_batch)
            .with_n_ubatch(params.n_ubatch)
            .with_n_seq_max(params.n_seq_max)
            .with_n_threads(params.n_threads)
            .with_n_threads_batch(params.n_threads_batch)
            .with_embeddings(params.embeddings)
            .with_offload_kqv(params.offload_kqv)
            .with_swa_full(params.swa_full)
            .with_type_k(params.type_k)
            .with_type_v(params.type_v)
            .with_rope_scaling_type(params.rope_scaling_type)
            .with_rope_freq_base(params.rope_freq_base)
            .with_rope_freq_scale(params.rope_freq_scale)
            .with_pooling_type(params.pooling_type);



        ctx_params
    }
}

/// Configuration for the LlamaModel.
#[derive(Debug, Clone)]
pub struct ModelParams {
    pub model_path: String,
    pub mmproj_path: Option<String>,

    pub n_gpu_layers: u32,
    pub main_gpu: i32,
    pub vocab_only: bool,
    pub use_mmap: bool,
    pub use_mlock: bool,
    pub split_mode: LlamaSplitMode,
}

impl Default for ModelParams {
    fn default() -> Self {
        Self {
            model_path: "".to_string(),
            mmproj_path: None,
            n_gpu_layers: 99,
            main_gpu: 0,
            vocab_only: false,
            use_mmap: true,
            use_mlock: false,
            split_mode: LlamaSplitMode::Layer,
        }
    }
}

impl From<&ModelParams> for LlamaModelParams {
    fn from(params: &ModelParams) -> Self {
        LlamaModelParams::default()
            .with_n_gpu_layers(params.n_gpu_layers)
            .with_main_gpu(params.main_gpu)
            .with_vocab_only(params.vocab_only)

            .with_use_mlock(params.use_mlock)
            .with_split_mode(params.split_mode)
    }
}

/// Configuration for Sampling.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct SamplerParams {
    pub temp: f32,
    pub top_k: i32,
    pub top_p: f32,
    pub min_p: f32,

    pub penalty_last_n: i32,
    pub penalty_repeat: f32,
    pub penalty_freq: f32,
    pub penalty_present: f32,

    pub use_mirostat: bool,
    pub tau: f32,
    pub eta: f32,

    pub seed: u32,
    pub grammar: Option<GrammarParams>,
}

impl Default for SamplerParams {
    fn default() -> Self {
        Self {
            temp: 0.80,
            top_k: 40,
            top_p: 0.95,
            min_p: 0.05,
            penalty_last_n: 64,
            penalty_repeat: 1.10,
            penalty_freq: 0.00,
            penalty_present: 0.00,
            use_mirostat: false,
            tau: 5.0,
            eta: 0.1,
            seed: 0xFFFFFFFF,
            grammar: None,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct GrammarParams {
    pub grammar: String,
    pub root: String,
}
