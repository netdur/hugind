use crate::core::config::server::*;
use crate::shared::paths;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::fs;

pub struct ConfigLoader;

impl ConfigLoader {
    pub fn load_server_config(path: &Path) -> Result<ServerConfig> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {:?}", path))?;

        let yaml: serde_yaml::Value = serde_yaml::from_str(&content)
            .with_context(|| "Failed to parse YAML")?;

        let name = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let server_section = yaml.get("server").unwrap_or(&serde_yaml::Value::Null);
        let model_section = yaml.get("model").unwrap_or(&serde_yaml::Value::Null);
        let context_section = yaml.get("context").unwrap_or(&serde_yaml::Value::Null);
        let sampling_section = yaml.get("sampling").unwrap_or(&serde_yaml::Value::Null);
        let chat_section = yaml.get("chat").unwrap_or(&serde_yaml::Value::Null);

        // Path resolution helper
        let resolve_path = |p: Option<&str>| -> Option<PathBuf> {
            p.map(|s| resolve_path_relative(s, path))
        };

        // Server Settings
        let host = server_section["host"].as_str().unwrap_or("0.0.0.0").to_string();
        let port = server_section["port"].as_u64().unwrap_or(8080) as u16;
        let api_key = server_section["api_key"]
            .as_str()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        
        let library_path = resolve_path(server_section["library_path"].as_str());
        
        // System Prompt
        let system_prompt_file = server_section["system_prompt_file"]
            .as_str()
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .map(|p| resolve_path_relative(p, path));
        let system_prompt = if let Some(p) = &system_prompt_file {
            fs::read_to_string(p).unwrap_or_else(|_| "You are a helpful assistant.".to_string())
        } else {
            "You are a helpful assistant.".to_string()
        };

        let embeddings_enabled = server_section["embeddings"].as_bool()
            .or_else(|| server_section["embeddings"].as_str().map(|s| s.eq_ignore_ascii_case("true")))
            .unwrap_or(false);

        let session_home = if let Some(s) = server_section["session_home"].as_str() {
             resolve_path_relative(s, path)
        } else {
             paths::sessions_dir()
        };

        // Model Settings
        let model_name = model_section["name"]
            .as_str()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let model_path_str = model_section["path"].as_str().unwrap_or("");
        let model_path = resolve_path_relative(model_path_str, path);
        
        if !model_path.exists() && !model_path_str.is_empty() {
             // In a real CLI we might error here, but for now let's just proceed or error if critical
             // Dart code throws exception
             anyhow::bail!("Model file not found at: {:?}", model_path);
        }

        let mmproj_path = resolve_path(model_section["mmproj_path"].as_str());
        
        // Parameter Tuning Logic
        let max_slots = server_section["max_slots"].as_u64().unwrap_or(4) as u32;

        let mut batch_size = context_section["batch_size"].as_u64().unwrap_or(
            if mmproj_path.is_some() { 8192 } else { 2048 }
        ) as u32;

        if mmproj_path.is_some() && batch_size < 8192 {
             println!("Vision model detected with low batch size. Auto-increasing to 8192.");
             batch_size = 8192;
        }

        let model_params = ModelParams {
            n_gpu_layers: model_section["gpu_layers"].as_u64().unwrap_or(99) as u32,
            split_mode: parse_split_mode(model_section["split_mode"].as_str()),
            main_gpu: model_section["main_gpu"].as_u64().unwrap_or(0) as u32,
            use_mmap: model_section["use_mmap"].as_bool().unwrap_or(true),
            use_mlock: model_section["use_mlock"].as_bool().unwrap_or(false),
            vocab_only: model_section["vocab_only"].as_bool().unwrap_or(false),
        };

        let context_params = ContextParams {
            n_ctx: context_section["size"].as_u64().unwrap_or(4096) as u32,
            n_batch: batch_size,
            n_ubatch: context_section["ubatch_size"].as_u64().unwrap_or(512) as u32,
            n_seq_max: max_slots, // Critical: set to max_slots
            n_threads: context_section["threads"].as_u64().unwrap_or(8) as u32,
            n_threads_batch: context_section["threads_batch"].as_u64().unwrap_or(8) as u32,
            flash_attention: parse_flash_attn(&context_section["flash_attention"]),
            type_k: parse_cache_type(context_section["cache_type_k"].as_str()),
            type_v: parse_cache_type(context_section["cache_type_v"].as_str()),
            offload_kqv: context_section["offload_kqv"].as_bool().unwrap_or(true),
            embeddings: embeddings_enabled,
        };

        let sampler_params = SamplerParams {
            temp: sampling_section["temp"].as_f64().unwrap_or(0.7) as f32,
            top_k: sampling_section["top_k"].as_u64().unwrap_or(40) as u32,
            top_p: sampling_section["top_p"].as_f64().unwrap_or(0.95) as f32,
            min_p: sampling_section["min_p"].as_f64().unwrap_or(0.05) as f32,
            dry_multiplier: sampling_section["dry_multiplier"].as_f64().unwrap_or(0.0) as f32,
            repeat_last_n: sampling_section["repeat_last_n"].as_u64().unwrap_or(64) as u32,
            repeat_penalty: sampling_section["repeat_penalty"].as_f64().unwrap_or(1.1) as f32,
            frequency_penalty: sampling_section["frequency_penalty"].as_f64().unwrap_or(0.0) as f32,
            presence_penalty: sampling_section["presence_penalty"].as_f64().unwrap_or(0.0) as f32,
            dry_base: sampling_section["dry_base"].as_f64().unwrap_or(1.75) as f32,
            dry_allowed_length: sampling_section["dry_allowed_length"].as_u64().unwrap_or(2) as u32,
            xtc_probability: sampling_section["xtc_probability"].as_f64().unwrap_or(0.0) as f32,
            xtc_threshold: sampling_section["xtc_threshold"].as_f64().unwrap_or(0.1) as f32,
        };

        let chat_format = parse_chat_format(chat_section["format"].as_str());

        Ok(ServerConfig {
            name,
            host,
            port,
            library_path,
            api_key,
            concurrency: server_section["concurrency"].as_u64().unwrap_or(1) as u32,
            max_slots,
            timeout_seconds: server_section["timeout_seconds"].as_u64().unwrap_or(600) as u64,
            system_prompt,
            system_prompt_file,
            embeddings_enabled,
            session_home,
            verbose: server_section["verbose"].as_bool().unwrap_or(false),
            model_path,
            mmproj_path,
            model_name,
            model_params,
            context_params,
            sampler_params,
            chat_format,
        })
    }
}

// Helpers
fn resolve_path_relative(raw_path: &str, config_path: &Path) -> PathBuf {
    if raw_path.is_empty() {
        return PathBuf::new();
    }
    
    let mut resolved = raw_path.to_string();
    
    // Handle ~ expansion
    if resolved.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            resolved = resolved.replacen("~", home.to_string_lossy().as_ref(), 1);
        }
    }

    let path = Path::new(&resolved);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        config_path.parent().unwrap_or(Path::new(".")).join(path)
    }
}

fn parse_split_mode(val: Option<&str>) -> SplitMode {
    match val {
        Some("none") => SplitMode::None,
        Some("layer") => SplitMode::Layer,
        Some("row") => SplitMode::Row,
        _ => SplitMode::Layer,
    }
}

fn parse_flash_attn(val: &serde_yaml::Value) -> bool {
    match val {
        serde_yaml::Value::Bool(b) => *b,
        serde_yaml::Value::String(s) => s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("enabled"),
        _ => false,
    }
}

fn parse_cache_type(val: Option<&str>) -> CacheType {
    match val {
        Some("f32") => CacheType::F32,
        Some("f16") => CacheType::F16,
        Some("q4_0") => CacheType::Q4_0,
        Some("q4_1") => CacheType::Q4_1,
        Some("q5_0") => CacheType::Q5_0,
        Some("q5_1") => CacheType::Q5_1,
        Some("q8_0") => CacheType::Q8_0,
        _ => CacheType::F16,
    }
}

fn parse_chat_format(val: Option<&str>) -> Option<ChatFormat> {
    match val.map(|s| s.to_lowercase()).as_deref() {
        Some("llama2") => Some(ChatFormat::Llama2),
        Some("chatml") => Some(ChatFormat::Chatml),
        Some("gemma") => Some(ChatFormat::Gemma),
        Some("vicuna") => Some(ChatFormat::Vicuna),
        Some("zephyr") => Some(ChatFormat::Zepyhr),
        Some("openchat") => Some(ChatFormat::Openchat),
        Some("deepseek") => Some(ChatFormat::Deepseek),
        Some("qwen3") => Some(ChatFormat::Qwen3),
        _ => None,
    }
}
