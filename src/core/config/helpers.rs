use crate::shared::{configs, paths};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::{Path, PathBuf};

/// Find a config file by name, checking both .yml and .yaml extensions.
pub fn find_config_path(name: &str) -> Option<PathBuf> {
    let config_dir = paths::configs_dir();
    let yml_path = config_dir.join(format!("{}.yml", name));
    let yaml_path = config_dir.join(format!("{}.yaml", name));
    if yml_path.exists() {
        Some(yml_path)
    } else if yaml_path.exists() {
        Some(yaml_path)
    } else {
        None
    }
}

/// List all user config names and paths (excludes reserved names).
pub fn list_config_names() -> Result<Vec<(String, PathBuf)>> {
    let config_dir = paths::configs_dir();
    if !config_dir.exists() {
        return Ok(vec![]);
    }

    let mut items = Vec::new();
    for entry in fs::read_dir(&config_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if ext == "yml" || ext == "yaml" {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if configs::is_reserved_config_name(stem) {
                            continue;
                        }
                        items.push((stem.to_string(), path));
                    }
                }
            }
        }
    }
    items.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(items)
}

/// Estimate KV cache bytes per token based on model file size.
///
/// Model file size is a reasonable proxy for parameter count under quantization.
/// The KV cache cost per token depends on model architecture (layers, head dim, kv heads).
/// Since we can't read GGUF metadata at config time, we use the file size to
/// estimate the model class and derive bytes-per-token for f16 KV cache with 1 slot.
///
/// Rough reference (f16 KV, 1 slot):
///   ~1-3B params (1-2 GB file):  ~0.3-0.5 MB/token
///   ~7-8B params (4-5 GB file):  ~0.8-1.0 MB/token
///   ~13B params  (7-8 GB file):  ~1.2-1.6 MB/token
///   ~30B+ params (15+ GB file):  ~2.0-3.0 MB/token
fn estimate_kv_bytes_per_token(model_size_gb: f64) -> f64 {
    // Linear interpolation: larger models have more layers and wider KV
    // Base: ~0.15 MB/token per GB of model file (empirical fit across common quants)
    let mb_per_token = (0.15 * model_size_gb).clamp(0.3, 4.0);
    mb_per_token * 1_048_576.0 // convert to bytes
}

/// Recommend a context size given available memory for KV cache and model file size.
///
/// `available_mem_bytes`: memory available for KV cache (after model + OS headroom)
/// `model_size_gb`: model file size in GB (proxy for architecture)
/// `n_slots`: number of concurrent sequences (multiplies KV usage)
pub fn recommend_ctx(available_mem_bytes: u64, model_size_gb: f64, n_slots: u32) -> u64 {
    let kv_per_token = estimate_kv_bytes_per_token(model_size_gb);
    let total_kv_per_token = kv_per_token * n_slots as f64;

    let max_tokens = if total_kv_per_token > 0.0 {
        (available_mem_bytes as f64 / total_kv_per_token) as u64
    } else {
        4096
    };

    // Snap down to the nearest power-of-two option
    let options: Vec<u64> = vec![
        2048, 4096, 8192, 16384, 32768, 65536, 131072, 262144,
    ];

    options
        .iter()
        .filter(|&&c| c <= max_tokens)
        .last()
        .copied()
        .unwrap_or(2048)
}

/// Compute how much memory is available for KV cache.
///
/// On unified memory (Apple Silicon): total_ram - model - OS headroom
/// On discrete GPU: GPU VRAM - model (if it fits), else fall back to RAM
pub fn available_mem_for_ctx(
    total_ram_bytes: u64,
    gpu_vram_bytes: Option<u64>,
    model_size_gb: f64,
    is_unified_memory: bool,
) -> u64 {
    let model_bytes = (model_size_gb * 1_073_741_824.0) as u64;
    let os_headroom = 2u64 * 1_073_741_824; // 2 GB for OS + runtime

    if is_unified_memory {
        // Apple Silicon: model + KV cache share system RAM
        total_ram_bytes.saturating_sub(model_bytes).saturating_sub(os_headroom)
    } else if let Some(vram) = gpu_vram_bytes {
        // Discrete GPU: model weights on VRAM, KV cache also on VRAM if offloaded
        let vram_after_model = vram.saturating_sub(model_bytes);
        if vram_after_model > 512 * 1_048_576 {
            // Enough VRAM left for KV cache
            vram_after_model
        } else {
            // Model barely fits in VRAM, KV falls back to RAM
            total_ram_bytes.saturating_sub(os_headroom)
        }
    } else {
        // CPU only: everything in RAM
        total_ram_bytes.saturating_sub(model_bytes).saturating_sub(os_headroom)
    }
}

/// Parse GPU memory string from nvidia-smi (e.g. "24576 MiB") into bytes.
pub fn parse_gpu_memory(memory_str: &str) -> Option<u64> {
    let s = memory_str.trim();
    if let Some(mib_str) = s.strip_suffix("MiB").or_else(|| s.strip_suffix("mib")) {
        mib_str.trim().parse::<u64>().ok().map(|m| m * 1_048_576)
    } else if let Some(gib_str) = s.strip_suffix("GiB").or_else(|| s.strip_suffix("gib")) {
        gib_str.trim().parse::<u64>().ok().map(|g| g * 1_073_741_824)
    } else if let Some(mb_str) = s.strip_suffix("MB").or_else(|| s.strip_suffix("mb")) {
        mb_str.trim().parse::<u64>().ok().map(|m| m * 1_000_000)
    } else {
        // Try parsing as plain number (assume MiB)
        s.parse::<u64>().ok().map(|m| m * 1_048_576)
    }
}

/// Detect a sibling file (e.g. vision projector) next to the main model file.
pub fn detect_sibling(main_path: &str, keywords: &[&str]) -> Option<String> {
    let path = Path::new(main_path);
    if !path.exists() {
        return None;
    }

    let parent = path.parent()?;
    let main_name = path.file_name()?.to_str()?.to_lowercase();

    if let Ok(entries) = fs::read_dir(parent) {
        for entry in entries.filter_map(Result::ok) {
            let p = entry.path();
            if p.is_file() {
                if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                    let name_lower = name.to_lowercase();
                    if name_lower != main_name && name_lower.ends_with(".gguf") {
                        if keywords.iter().any(|k| name_lower.contains(k)) {
                            return Some(p.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

/// Shorten a path by replacing the home directory with ~.
pub fn shorten_path(path: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        if path.starts_with(home_str.as_ref()) {
            return path.replacen(home_str.as_ref(), "~", 1);
        }
    }
    path.to_string()
}

/// Validate a config name: must be non-empty, alphanumeric/dash/underscore, not reserved, no path traversal.
pub fn validate_config_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("Config name cannot be empty"));
    }
    if configs::is_reserved_config_name(name) {
        return Err(anyhow!("Config name '{}' is reserved", name));
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err(anyhow!("Config name '{}' contains invalid characters", name));
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return Err(anyhow!(
            "Config name '{}' must only contain alphanumeric characters, dashes, underscores, or dots",
            name
        ));
    }
    Ok(())
}

/// Mask a sensitive value for display, showing only the last 4 characters.
pub fn mask_sensitive_value(value: &str) -> String {
    if value.len() <= 4 {
        return "****".to_string();
    }
    let visible = &value[value.len() - 4..];
    format!("****{}", visible)
}
