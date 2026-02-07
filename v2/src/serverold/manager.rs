use std::sync::Arc;

use anyhow::{Context, Result};
use llama_cpp_2::context::params::KvCacheType;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::model::params::LlamaSplitMode;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::token::LlamaToken;
use std::time::Duration;

use crate::core::config::server as core_config;
use crate::server::llm_backend::config as backend_config;
use crate::server::llm_backend::service::{LlamaService, SendStatus, StreamEvent};

pub struct ServerManager {
    config: core_config::ServerConfig,
    model_name: String,
    backend: &'static LlamaBackend,
    model: &'static LlamaModel,
    service: Arc<LlamaService<'static>>,
    sampler_params: backend_config::SamplerParams,
}

// SAFETY: Manager holds llama.cpp objects that are not marked Send/Sync.
// We serialize access through the service's internal mutexes and treat
// the manager as a single-instance global for the process.
unsafe impl Send for ServerManager {}
unsafe impl Sync for ServerManager {}

impl ServerManager {
    pub fn new(config: core_config::ServerConfig) -> Result<Self> {
        let model_name = config
            .model_name
            .clone()
            .unwrap_or_else(|| config.name.clone());

        let mut backend = LlamaBackend::init()
            .context("Failed to initialize llama backend")?;
        if !config.verbose {
            backend.void_logs();
        }

        // NOTE: LlamaService holds references to backend/model. We leak them to give the service
        // a stable lifetime for the process duration (server-style usage).
        let backend = Box::leak(Box::new(backend));

        let model_params = map_model_params(&config);
        let model = LlamaModel::load_from_file(
            backend,
            &config.model_path,
            &(&model_params).into(),
        )
        .with_context(|| format!("Failed to load model: {:?}", config.model_path))?;
        let model = Box::leak(Box::new(model));

        let ctx_params = map_context_params(&config);
        let service = LlamaService::new(backend, model, &ctx_params, &model_params)
            .map_err(|e| anyhow::anyhow!(e.to_string()))
            .context("Failed to initialize LlamaService")?;

        let sampler_params = map_sampler_params(&config);

        Ok(Self {
            config,
            model_name,
            backend,
            model,
            service: Arc::new(service),
            sampler_params,
        })
    }

    pub fn service(&self) -> Arc<LlamaService<'static>> {
        Arc::clone(&self.service)
    }

    pub fn sampler_params(&self) -> &backend_config::SamplerParams {
        &self.sampler_params
    }

    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    pub fn config(&self) -> &core_config::ServerConfig {
        &self.config
    }

    pub fn backend(&self) -> &'static LlamaBackend {
        self.backend
    }

    pub fn model(&self) -> &'static LlamaModel {
        self.model
    }

    pub fn start_heartbeat(self: &Arc<Self>) {
        let service = self.service.clone();
        tokio::task::spawn_blocking(move || {
            let mut completed: Vec<(String, LlamaToken)> = Vec::new();
            loop {
                if !service.has_pending_work() {
                    service.wait_for_work();
                    continue;
                }

                let step = match service.service_step(&completed) {
                    Ok(step) => step,
                    Err(e) => {
                        eprintln!("heartbeat service_step failed: {}", e);
                        completed.clear();
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                };

                completed = step.completed.clone();

                for (session_id, token) in step.emitted {
                    if let Ok(piece) = service.decode_token(&session_id, token) {
                        match service.send_event(&session_id, StreamEvent::Token(piece)) {
                            SendStatus::Sent => {}
                            SendStatus::NoListener => {}
                            SendStatus::Closed => {
                                println!("[stream] client disconnected: {}", session_id);
                                let _ = service.cancel_session(&session_id);
                            }
                        }
                    }
                }

                for (session_id, token) in &completed {
                    if service.is_eog_token(*token) {
                        let _ = service.send_event(session_id, StreamEvent::Done);
                        service.unregister_stream(session_id);
                    }
                }
            }
        });
    }
}

fn map_model_params(config: &core_config::ServerConfig) -> backend_config::ModelParams {
    let cfg = &config.model_params;
    backend_config::ModelParams {
        model_path: config.model_path.to_string_lossy().to_string(),
        mmproj_path: config.mmproj_path.as_ref().map(|p| p.to_string_lossy().to_string()),
        n_gpu_layers: cfg.n_gpu_layers,
        main_gpu: cfg.main_gpu as i32,
        vocab_only: cfg.vocab_only,
        use_mmap: cfg.use_mmap,
        use_mlock: cfg.use_mlock,
        split_mode: map_split_mode(cfg.split_mode),
    }
}

fn map_context_params(config: &core_config::ServerConfig) -> backend_config::ContextParams {
    let cfg = &config.context_params;
    let mut params = backend_config::ContextParams::default();
    params.n_ctx = cfg.n_ctx;
    params.n_batch = cfg.n_batch;
    params.n_ubatch = cfg.n_ubatch;
    params.n_seq_max = cfg.n_seq_max;
    params.n_threads = cfg.n_threads as i32;
    params.n_threads_batch = cfg.n_threads_batch as i32;
    params.flash_attention = cfg.flash_attention;
    params.type_k = map_cache_type(cfg.type_k);
    params.type_v = map_cache_type(cfg.type_v);
    params.offload_kqv = cfg.offload_kqv;
    params.embeddings = cfg.embeddings;
    params
}

fn map_sampler_params(config: &core_config::ServerConfig) -> backend_config::SamplerParams {
    let cfg = &config.sampler_params;
    let mut params = backend_config::SamplerParams::default();
    params.temp = cfg.temp;
    params.top_k = cfg.top_k as i32;
    params.top_p = cfg.top_p;
    params.min_p = cfg.min_p;
    params.penalty_last_n = cfg.repeat_last_n as i32;
    params.penalty_repeat = cfg.repeat_penalty;
    params.penalty_freq = cfg.frequency_penalty;
    params.penalty_present = cfg.presence_penalty;
    params
}

fn map_split_mode(mode: core_config::SplitMode) -> LlamaSplitMode {
    match mode {
        core_config::SplitMode::None => LlamaSplitMode::None,
        core_config::SplitMode::Layer => LlamaSplitMode::Layer,
        core_config::SplitMode::Row => LlamaSplitMode::Row,
    }
}

fn map_cache_type(cache: core_config::CacheType) -> KvCacheType {
    match cache {
        core_config::CacheType::F32 => KvCacheType::F32,
        core_config::CacheType::F16 => KvCacheType::F16,
        core_config::CacheType::Q4_0 => KvCacheType::Q4_0,
        core_config::CacheType::Q4_1 => KvCacheType::Q4_1,
        core_config::CacheType::Q5_0 => KvCacheType::Q5_0,
        core_config::CacheType::Q5_1 => KvCacheType::Q5_1,
        core_config::CacheType::Q8_0 => KvCacheType::Q8_0,
    }
}
