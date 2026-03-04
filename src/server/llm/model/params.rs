#[derive(Debug, Clone)]
pub struct ModelParams {
    pub n_gpu_layers: i32,
    pub split_mode: llama_cpp::llama_split_mode,
    pub main_gpu: i32,
    pub tensor_split: Option<Vec<f32>>,
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
        unsafe {
            let defaults = llama_cpp::llama_model_default_params();
            Self {
                n_gpu_layers: defaults.n_gpu_layers,
                split_mode: defaults.split_mode,
                main_gpu: defaults.main_gpu,
                tensor_split: None,
                vocab_only: defaults.vocab_only,
                use_mmap: defaults.use_mmap,
                use_direct_io: defaults.use_direct_io,
                use_mlock: defaults.use_mlock,
                check_tensors: defaults.check_tensors,
                use_extra_bufts: defaults.use_extra_bufts,
                no_host: defaults.no_host,
                no_alloc: defaults.no_alloc,
            }
        }
    }
}

impl ModelParams {
    pub fn to_c_params(&self) -> llama_cpp::llama_model_params {
        unsafe {
            let mut params = llama_cpp::llama_model_default_params();
            params.n_gpu_layers = self.n_gpu_layers;
            params.split_mode = self.split_mode;
            params.main_gpu = self.main_gpu;

            if let Some(ts) = &self.tensor_split {
                params.tensor_split = ts.as_ptr();
            }

            params.vocab_only = self.vocab_only;
            params.use_mmap = self.use_mmap;
            params.use_direct_io = self.use_direct_io;
            params.use_mlock = self.use_mlock;
            params.check_tensors = self.check_tensors;
            params.use_extra_bufts = self.use_extra_bufts;
            params.no_host = self.no_host;
            params.no_alloc = self.no_alloc;
            params
        }
    }
}
