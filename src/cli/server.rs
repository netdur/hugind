use anyhow::{Context, Result};
use serde_json::Value as JsonValue;
use serde_yaml::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::shared::{paths, configs};
use crate::engine::{LlmEngine, request::Request, types::EventKind, EngineStats};
use crate::llm::{model::{Model, ModelParams}, context::ContextParams, runtime};
use crate::server;

pub async fn run_start(_config: String, _port: Option<u16>) -> Result<()> {
    runtime::init();
    runtime::logging::init_silent_logging();

    
    let path = find_config_path(&_config)
        .ok_or_else(|| anyhow::anyhow!("Config \"{}\" not found.", _config))?;

    let mut cfg = crate::core::config::loader::ConfigLoader::load_server_config(&path)?;
    if let Some(port) = _port {
        cfg.port = port;
    }

    
    runtime::init();
    
    
    println!("Loading model from {:?}", cfg.model_path);

    
    
    
    let mut mparams = ModelParams::default();
    mparams.n_gpu_layers = cfg.model_params.n_gpu_layers as i32;
    mparams.main_gpu = cfg.model_params.main_gpu as i32;
    mparams.use_mmap = cfg.model_params.use_mmap;
    mparams.use_mlock = cfg.model_params.use_mlock;
    
    
    let model = Arc::new(Model::from_file(cfg.model_path.to_str().unwrap(), &mparams)?);
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
    cparams.n_seq_max = cfg.max_slots; 
    cparams.embeddings = cfg.embeddings_enabled;
    if cfg.embeddings_enabled {
        cparams.pooling_type = llama_cpp::llama_pooling_type_LLAMA_POOLING_TYPE_MEAN;
    }

    
    let (engine_tx, engine_rx) = mpsc::channel::<Request>(32);
    let kv_manager = Arc::new(crate::engine::kv_cache::KvCacheManager::new(cfg.unified_memory_mode));
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
        
        
        let mut engine = match LlmEngine::new(&engine_model, &cparams, mm_path_ref, engine_rx, kv_manager, engine_stats) {
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
                            },
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
        cfg.host.clone(), 
        cfg.port, 
        cfg.api_key.clone()
    ).await;
    
    Ok(())
}

pub async fn run_list() -> Result<()> {
    let config_dir = paths::configs_dir();
    if !config_dir.exists() {
        println!("No configs found (directory does not exist: {:?}).", config_dir);
        return Ok(());
    }

    let configs = list_config_files(&config_dir)?;
    if configs.is_empty() {
        println!("No configs found.");
        return Ok(());
    }

    println!("Saved Configs:");
    for path in configs {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        let (host, port) = read_host_port(&path).unwrap_or(("127.0.0.1".to_string(), 8080));
        let monitor_url = format!("http://{}:{}/v1/monitor", normalize_host(&host), port);
        let info = fetch_monitor_info(&monitor_url).await;
        let status = format_status(info.as_ref(), name);
        println!("- {} ({})", name, status);
    }

    Ok(())
}

pub async fn run_stop(config: String) -> Result<()> {
    let path = find_config_path(&config)
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

fn list_config_files(config_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut configs = Vec::new();
    for entry in fs::read_dir(config_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if ext == "yml" || ext == "yaml" {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if configs::is_reserved_config_name(stem) {
                            continue;
                        }
                    }
                    configs.push(path);
                }
            }
        }
    }
    Ok(configs)
}

fn find_config_path(config: &str) -> Option<PathBuf> {
    let config_dir = paths::configs_dir();
    let yml_path = config_dir.join(format!("{}.yml", config));
    let yaml_path = config_dir.join(format!("{}.yaml", config));
    if yml_path.exists() {
        Some(yml_path)
    } else if yaml_path.exists() {
        Some(yaml_path)
    } else {
        None
    }
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
    let port = server
        .get("port")
        .and_then(|p| p.as_u64())
        .unwrap_or(8080) as u16;
    Ok((host, port))
}

fn normalize_host(host: &str) -> &str {
    if host == "0.0.0.0" {
        "127.0.0.1"
    } else {
        host
    }
}

fn kill_by_port(port: u16) -> Result<Vec<i32>> {
    use std::process::Command;

    let output = Command::new("lsof")
        .arg("-ti")
        .arg(format!("tcp:{}", port))
        .output()
        .with_context(|| "Failed to run lsof")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let pids: Vec<i32> = stdout
        .lines()
        .filter_map(|line| line.trim().parse::<i32>().ok())
        .collect();

    if pids.is_empty() {
        return Ok(Vec::new());
    }

    for pid in &pids {
        let _ = Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .status();
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
    let config_name = json.get("config_name").and_then(|v| v.as_str()).map(|s| s.to_string());
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
