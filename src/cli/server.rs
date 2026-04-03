use anyhow::{Context, Result};
use parking_lot::RwLock;
use serde_json::Value as JsonValue;
use serde_yaml::Value;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::core::config::server::{
    AttentionType, CacheType, FlashAttnType, PoolingType, RopeScalingType, SamplerParams, SplitMode,
};
use crate::engine::{EngineStats, LlmEngine, request::Request, types::EventKind};
use crate::llm::{
    context::ContextParams,
    model::{Model, ModelParams},
    runtime,
    sampling::SamplingConfig,
};
use crate::server;
use crate::core::config::helpers as config_helpers;

pub async fn run_start(_config: String, _port: Option<u16>) -> Result<()> {
    runtime::init();
    runtime::logging::init_silent_logging();

    let path = config_helpers::find_config_path(&_config)
        .ok_or_else(|| anyhow::anyhow!("Config \"{}\" not found.", _config))?;

    let mut cfg = crate::core::config::loader::ConfigLoader::load_server_config(&path)?;
    if let Some(port) = _port {
        cfg.port = port;
    }

    println!("Loading model from {:?}", cfg.model_path);

    let mut mparams = ModelParams::default();
    mparams.n_gpu_layers = cfg.model_params.n_gpu_layers;
    mparams.split_mode = map_split_mode(cfg.model_params.split_mode);
    mparams.main_gpu = cfg.model_params.main_gpu;
    mparams.tensor_split = if cfg.model_params.tensor_split.is_empty() {
        None
    } else {
        Some(cfg.model_params.tensor_split.clone())
    };
    mparams.vocab_only = cfg.model_params.vocab_only;
    mparams.use_mmap = cfg.model_params.use_mmap;
    mparams.use_direct_io = cfg.model_params.use_direct_io;
    mparams.use_mlock = cfg.model_params.use_mlock;
    mparams.check_tensors = cfg.model_params.check_tensors;
    mparams.use_extra_bufts = cfg.model_params.use_extra_bufts;
    mparams.no_host = cfg.model_params.no_host;
    mparams.no_alloc = cfg.model_params.no_alloc;

    let model_path_str = cfg.model_path.to_str()
        .ok_or_else(|| anyhow::anyhow!("Model path contains invalid UTF-8: {:?}", cfg.model_path))?;
    let model = Arc::new(Model::from_file(model_path_str, &mparams).map_err(|e| {
        let logs = runtime::logging::drain_log_buffer();
        let log_output: String = logs.into_iter().collect();
        let log_section = if log_output.trim().is_empty() {
            String::new()
        } else {
            format!("\n\nBackend logs:\n{}", log_output.trim())
        };
        anyhow::anyhow!("{}{}", e, log_section)
    })?);
    let model_name = cfg.model_name.clone().or_else(|| {
        cfg.model_path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
    });
    let config_name = Some(cfg.name.clone());

    let mut cparams = ContextParams::default();
    cparams.n_ctx = cfg.context_params.n_ctx;
    cparams.n_batch = cfg.context_params.n_batch;
    cparams.n_ubatch = cfg.context_params.n_ubatch;
    cparams.n_seq_max = cfg.context_params.n_seq_max;
    cparams.n_threads = cfg.context_params.n_threads;
    cparams.n_threads_batch = cfg.context_params.n_threads_batch;
    cparams.rope_scaling_type = map_rope_scaling_type(cfg.context_params.rope_scaling_type);
    cparams.pooling_type = map_pooling_type(cfg.context_params.pooling_type);
    cparams.attention_type = map_attention_type(cfg.context_params.attention_type);
    cparams.flash_attn_type = map_flash_attn_type(cfg.context_params.flash_attn_type);
    cparams.rope_freq_base = cfg.context_params.rope_freq_base;
    cparams.rope_freq_scale = cfg.context_params.rope_freq_scale;
    cparams.yarn_ext_factor = cfg.context_params.yarn_ext_factor;
    cparams.yarn_attn_factor = cfg.context_params.yarn_attn_factor;
    cparams.yarn_beta_fast = cfg.context_params.yarn_beta_fast;
    cparams.yarn_beta_slow = cfg.context_params.yarn_beta_slow;
    cparams.yarn_orig_ctx = cfg.context_params.yarn_orig_ctx;
    cparams.defrag_thold = cfg.context_params.defrag_thold;
    cparams.type_k = map_cache_type(cfg.context_params.type_k);
    cparams.type_v = map_cache_type(cfg.context_params.type_v);
    cparams.embeddings = cfg.embeddings_enabled;
    cparams.offload_kqv = cfg.context_params.offload_kqv;
    cparams.no_perf = cfg.context_params.no_perf;
    cparams.op_offload = cfg.context_params.op_offload;
    cparams.swa_full = cfg.context_params.swa_full;
    cparams.kv_unified = cfg.context_params.kv_unified;
    if cfg.embeddings_enabled {
        cparams.pooling_type = map_pooling_type(PoolingType::Mean);
        cparams.n_ubatch = cparams.n_batch;
    }

    let sampling_defaults = map_sampling_config(&cfg.sampler_params);
    let system_prompt = {
        let trimmed = cfg.system_prompt.trim();
        if trimmed.is_empty() || trimmed == "You are a helpful assistant." {
            None
        } else {
            Some(trimmed.to_string())
        }
    };

    let (engine_tx, engine_rx) = mpsc::channel::<Request>(32);
    let kv_manager = Arc::new(crate::engine::kv_cache::KvCacheManager::new(
        cfg.unified_memory_mode,
    ));
    let engine_stats = Arc::new(RwLock::new(EngineStats::default()));

    let engine_model = model.clone();
    let mmproj = cfg.mmproj_path.clone();
    let server_kv_manager = kv_manager.clone();
    let server_engine_stats = engine_stats.clone();
    std::thread::spawn(move || {
        println!("Engine thread started");

        let mm_path_str = mmproj.as_ref().map(|p| p.to_str().unwrap_or(""));
        let mm_path_ref = if let Some(s) = mm_path_str {
            if s.is_empty() { None } else { Some(s) }
        } else {
            None
        };

        let mut engine = match LlmEngine::new(
            &engine_model,
            &cparams,
            mm_path_ref,
            engine_rx,
            kv_manager,
            engine_stats,
        ) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Failed to initialize engine: {}", e);
                return;
            }
        };

        println!("Engine initialized, entering loop");
        loop {
            match engine.pull() {
                Ok(events) => {
                    for event in events {
                        match event.kind {
                            EventKind::Text { .. } => {}
                            EventKind::Finish { .. } => {}
                            EventKind::Error { message, request } => {
                                eprintln!("[{}] Error: {}", request.id(), message);
                            }
                            EventKind::Embedding { .. } => {}
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Engine pull error: {}", e);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
    });

    println!("Starting Server on {}:{}", cfg.host, cfg.port);
    server::run_server(
        engine_tx,
        server_kv_manager,
        server_engine_stats,
        model.clone(),
        model_name,
        config_name,
        cfg.embeddings_enabled,
        cfg.enable_thinking_default,
        cfg.thinking_budget_tokens,
        sampling_defaults,
        system_prompt,
        cfg.host.clone(),
        cfg.port,
        cfg.api_key.clone(),
    )
    .await;

    Ok(())
}

pub async fn run_list() -> Result<()> {
    let items = config_helpers::list_config_names()?;
    if items.is_empty() {
        println!("No configs found.");
        return Ok(());
    }

    println!("Saved Configs:");
    for (name, path) in &items {
        let (host, port) = read_host_port(path).unwrap_or(("127.0.0.1".to_string(), 8080));
        let monitor_url = format!("http://{}:{}/v1/monitor", normalize_host(&host), port);
        let info = fetch_monitor_info(&monitor_url).await;
        let status = format_status(info.as_ref(), &name);
        println!("- {} ({})", name, status);
    }

    Ok(())
}

pub async fn run_stop(config: String) -> Result<()> {
    let path = config_helpers::find_config_path(&config)
        .ok_or_else(|| anyhow::anyhow!("Config \"{}\" not found.", config))?;
    let (host, port) = read_host_port(&path).unwrap_or(("127.0.0.1".to_string(), 8080));
    let monitor_url = format!("http://{}:{}/v1/monitor", normalize_host(&host), port);
    let info = fetch_monitor_info(&monitor_url).await;
    if info.is_some() {
        println!("Server appears healthy at {}.", monitor_url);
    } else {
        println!("Server not reachable at {}.", monitor_url);
    }

    if cfg!(target_os = "windows") {
        println!("Stop not implemented on Windows yet.");
        return Ok(());
    }

    let killed = kill_by_port(port)?;
    if killed.is_empty() {
        println!("No processes found listening on port {}.", port);
    } else {
        println!("Stopped process(es) on port {}: {:?}", port, killed);
    }

    let info = fetch_monitor_info(&monitor_url).await;
    if info.is_some() {
        println!("Server still reachable at {}.", monitor_url);
    } else {
        println!("Server is down at {}.", monitor_url);
    }

    Ok(())
}

fn read_host_port(path: &Path) -> Result<(String, u16)> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {:?}", path))?;
    let yaml: Value = serde_yaml::from_str(&content).with_context(|| "Failed to parse YAML")?;
    let server = yaml.get("server").unwrap_or(&Value::Null);
    let host = server
        .get("host")
        .and_then(|h| h.as_str())
        .unwrap_or("127.0.0.1")
        .to_string();
    let port = server.get("port").and_then(|p| p.as_u64()).unwrap_or(8080) as u16;
    Ok((host, port))
}

fn normalize_host(host: &str) -> &str {
    if host == "0.0.0.0" { "127.0.0.1" } else { host }
}

fn map_split_mode(mode: SplitMode) -> llama_cpp::llama_split_mode {
    (match mode {
        SplitMode::None => 0,
        SplitMode::Layer => 1,
        SplitMode::Row => 2,
    }) as llama_cpp::llama_split_mode
}

fn map_rope_scaling_type(value: RopeScalingType) -> llama_cpp::llama_rope_scaling_type {
    (match value {
        RopeScalingType::Unspecified => -1,
        RopeScalingType::None => 0,
        RopeScalingType::Linear => 1,
        RopeScalingType::Yarn => 2,
        RopeScalingType::Longrope => 3,
    }) as llama_cpp::llama_rope_scaling_type
}

fn map_pooling_type(value: PoolingType) -> llama_cpp::llama_pooling_type {
    (match value {
        PoolingType::Unspecified => -1,
        PoolingType::None => 0,
        PoolingType::Mean => 1,
        PoolingType::Cls => 2,
        PoolingType::Last => 3,
        PoolingType::Rank => 4,
    }) as llama_cpp::llama_pooling_type
}

fn map_attention_type(value: AttentionType) -> llama_cpp::llama_attention_type {
    (match value {
        AttentionType::Unspecified => -1,
        AttentionType::Causal => 0,
        AttentionType::NonCausal => 1,
    }) as llama_cpp::llama_attention_type
}

fn map_flash_attn_type(value: FlashAttnType) -> llama_cpp::llama_flash_attn_type {
    (match value {
        FlashAttnType::Auto => -1,
        FlashAttnType::On => 1,
        FlashAttnType::Off => 0,
    }) as llama_cpp::llama_flash_attn_type
}

fn map_cache_type(value: CacheType) -> llama_cpp::ggml_type {
    (match value {
        CacheType::F32 => 0,
        CacheType::F16 => 1,
        CacheType::Q4_0 => 2,
        CacheType::Q4_1 => 3,
        CacheType::Q5_0 => 6,
        CacheType::Q5_1 => 7,
        CacheType::Q8_0 => 8,
    }) as llama_cpp::ggml_type
}

fn map_sampling_config(value: &SamplerParams) -> SamplingConfig {
    let mut sampling = SamplingConfig::default();
    sampling.temp = value.temp;
    sampling.top_k = value.top_k;
    sampling.top_p = value.top_p;
    sampling.min_p = value.min_p;
    sampling.penalty_last_n = value.repeat_last_n;
    sampling.penalty_repeat = value.repeat_penalty;
    sampling
}

fn kill_by_port(port: u16) -> Result<Vec<i32>> {
    use std::process::Command;

    if port < 1024 {
        anyhow::bail!("Refusing to kill processes on privileged port {}", port);
    }

    let output = Command::new("lsof")
        .arg("-ti")
        .arg(format!("tcp:{}", port))
        .output()
        .with_context(|| "Failed to run lsof")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let self_pid = std::process::id() as i32;
    let pids: Vec<i32> = stdout
        .lines()
        .filter_map(|line| line.trim().parse::<i32>().ok())
        .filter(|pid| *pid > 0 && *pid != self_pid)
        .collect();

    if pids.is_empty() {
        return Ok(Vec::new());
    }

    // Send SIGTERM first, give processes a chance to shut down gracefully
    for pid in &pids {
        let _ = Command::new("kill").arg("-15").arg(pid.to_string()).status();
    }

    // Wait briefly then check if they're gone
    std::thread::sleep(std::time::Duration::from_millis(500));

    let mut remaining = Vec::new();
    for pid in &pids {
        // Check if process still exists
        let status = Command::new("kill").arg("-0").arg(pid.to_string()).status();
        if let Ok(s) = status {
            if s.success() {
                remaining.push(*pid);
            }
        }
    }

    // Force-kill any that didn't respond to SIGTERM
    for pid in &remaining {
        let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
    }

    Ok(pids)
}

async fn fetch_monitor_info(url: &str) -> Option<MonitorInfo> {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
    {
        Ok(c) => c,
        Err(_) => return None,
    };

    let resp = client.get(url).send().await.ok()?;
    let resp = resp.error_for_status().ok()?;
    let body = resp.text().await.ok()?;
    let json = serde_json::from_str::<JsonValue>(&body).ok()?;
    let config_name = json
        .get("config_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    Some(MonitorInfo { config_name })
}

fn format_status(info: Option<&MonitorInfo>, expected: &str) -> String {
    let Some(info) = info else {
        return "down".to_string();
    };

    if let Some(config_name) = info.config_name.as_deref() {
        if config_name == expected {
            return "up".to_string();
        }
        return "down".to_string();
    }

    "up".to_string()
}

struct MonitorInfo {
    config_name: Option<String>,
}
