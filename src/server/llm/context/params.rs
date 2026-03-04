#[derive(Debug, Clone)]
pub struct ContextParams {
    pub n_ctx: u32,
    pub n_batch: u32,
    pub n_ubatch: u32,
    pub n_seq_max: u32,
    pub n_threads: i32,
    pub n_threads_batch: i32,
    pub rope_scaling_type: llama_cpp::llama_rope_scaling_type,
    pub pooling_type: llama_cpp::llama_pooling_type,
    pub attention_type: llama_cpp::llama_attention_type,
    pub flash_attn_type: llama_cpp::llama_flash_attn_type,
    pub rope_freq_base: f32,
    pub rope_freq_scale: f32,
    pub yarn_ext_factor: f32,
    pub yarn_attn_factor: f32,
    pub yarn_beta_fast: f32,
    pub yarn_beta_slow: f32,
    pub yarn_orig_ctx: u32,
    pub defrag_thold: f32,
    pub cb_eval: llama_cpp::ggml_backend_sched_eval_callback,
    pub cb_eval_user_data: *mut std::ffi::c_void,
    pub type_k: llama_cpp::ggml_type,
    pub type_v: llama_cpp::ggml_type,

    pub embeddings: bool,
    pub offload_kqv: bool,
    pub op_offload: bool,
    pub swa_full: bool,
    pub kv_unified: bool,

    pub no_perf: bool,
}

impl Default for ContextParams {
    fn default() -> Self {
        unsafe {
            let defaults = llama_cpp::llama_context_default_params();
            Self {
                n_ctx: defaults.n_ctx,
                n_batch: defaults.n_batch,
                n_ubatch: defaults.n_ubatch,
                n_seq_max: defaults.n_seq_max,
                n_threads: defaults.n_threads,
                n_threads_batch: defaults.n_threads_batch,
                rope_scaling_type: defaults.rope_scaling_type,
                pooling_type: defaults.pooling_type,
                attention_type: defaults.attention_type,
                flash_attn_type: defaults.flash_attn_type,
                rope_freq_base: defaults.rope_freq_base,
                rope_freq_scale: defaults.rope_freq_scale,
                yarn_ext_factor: defaults.yarn_ext_factor,
                yarn_attn_factor: defaults.yarn_attn_factor,
                yarn_beta_fast: defaults.yarn_beta_fast,
                yarn_beta_slow: defaults.yarn_beta_slow,
                yarn_orig_ctx: defaults.yarn_orig_ctx,
                defrag_thold: defaults.defrag_thold,
                cb_eval: defaults.cb_eval,
                cb_eval_user_data: defaults.cb_eval_user_data,
                type_k: defaults.type_k,
                type_v: defaults.type_v,

                embeddings: defaults.embeddings,
                offload_kqv: defaults.offload_kqv,
                op_offload: defaults.op_offload,
                swa_full: defaults.swa_full,
                kv_unified: defaults.kv_unified,

                no_perf: defaults.no_perf,
            }
        }
    }
}

impl ContextParams {
    pub fn to_c_params(&self) -> llama_cpp::llama_context_params {
        unsafe {
            let mut params = llama_cpp::llama_context_default_params();
            params.n_ctx = self.n_ctx;
            params.n_batch = self.n_batch;
            params.n_ubatch = self.n_ubatch;
            params.n_seq_max = self.n_seq_max;
            params.n_threads = self.n_threads;
            params.n_threads_batch = self.n_threads_batch;
            params.rope_scaling_type = self.rope_scaling_type;
            params.pooling_type = self.pooling_type;
            params.attention_type = self.attention_type;
            params.flash_attn_type = self.flash_attn_type;
            params.rope_freq_base = self.rope_freq_base;
            params.rope_freq_scale = self.rope_freq_scale;
            params.yarn_ext_factor = self.yarn_ext_factor;
            params.yarn_attn_factor = self.yarn_attn_factor;
            params.yarn_beta_fast = self.yarn_beta_fast;
            params.yarn_beta_slow = self.yarn_beta_slow;
            params.yarn_orig_ctx = self.yarn_orig_ctx;
            params.defrag_thold = self.defrag_thold;
            params.cb_eval = self.cb_eval;
            params.cb_eval_user_data = self.cb_eval_user_data;
            params.type_k = self.type_k;
            params.type_v = self.type_v;

            params.embeddings = self.embeddings;
            params.offload_kqv = self.offload_kqv;
            params.op_offload = self.op_offload;
            params.swa_full = self.swa_full;
            params.kv_unified = self.kv_unified;

            params.no_perf = self.no_perf;
            params
        }
    }
}

unsafe impl Send for ContextParams {}
