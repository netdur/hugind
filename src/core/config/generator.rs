use crate::core::config::helpers;
use crate::core::sys::SystemInspector;
use crate::shared::paths;
use anyhow::{Result, anyhow};
use std::fs;

/// Parameters for generating a server config.
pub struct ConfigGenParams {
    pub name: String,
    pub model_path: String,
    pub ctx: Option<u64>,
    pub mmproj_path: Option<String>,
    pub overwrite: bool,
    /// Number of concurrent slots (default 4)
    pub n_slots: Option<u32>,
    /// Enable auto-fit (let the engine adjust context to fit memory at startup)
    pub enable_fit: bool,
    /// Per-device VRAM budget in MiB for fit (empty = no cap)
    pub fit_target_mib: Vec<usize>,
}

/// Result of config generation.
pub struct ConfigGenResult {
    pub path: String,
    pub model_path: String,
    pub ctx: u64,
    pub mmproj_path: Option<String>,
}

/// Generate a server config file from the base template, tuned for detected hardware.
pub fn generate_config(params: ConfigGenParams) -> Result<ConfigGenResult> {
    helpers::validate_config_name(&params.name)?;

    let info = SystemInspector::inspect();

    let base_yaml = include_str!("../../resources/config.yml");
    let mut base: serde_yaml::Value = serde_yaml::from_str(base_yaml)?;

    // Set model path
    let model_display = helpers::shorten_path(&params.model_path);
    set_nested(&mut base, &["model", "path"], serde_yaml::Value::String(model_display.clone()));

    // Auto-detect mmproj if not provided
    let mmproj_path = params
        .mmproj_path
        .or_else(|| helpers::detect_sibling(&params.model_path, &["mmproj", "projector", "vision"]));

    if let Some(ref mm) = mmproj_path {
        let mm_display = helpers::shorten_path(mm);
        set_nested(&mut base, &["model", "mmproj_path"], serde_yaml::Value::String(mm_display));
        set_nested(&mut base, &["context", "batch_size"], serde_yaml::Value::Number(8192.into()));
    }

    let model_size_gb = if let Ok(meta) = fs::metadata(&params.model_path) {
        meta.len() as f64 / 1_073_741_824.0
    } else {
        0.0
    };

    // --- Hardware-aware tuning ---
    let has_nvidia = info.gpus.iter().any(|g| g.name.to_lowercase().contains("nvidia"));
    let is_apple_silicon = cfg!(target_os = "macos") && std::env::consts::ARCH == "aarch64";
    let is_unified_memory = is_apple_silicon;

    // GPU layers
    let gpu_layers: i32 = if has_nvidia || is_apple_silicon { 99 } else { 0 };
    set_nested(&mut base, &["model", "gpu_layers"], serde_yaml::Value::Number(gpu_layers.into()));

    // Flash attention (enable for CUDA, auto for Metal)
    if has_nvidia {
        set_nested(&mut base, &["context", "flash_attention"], serde_yaml::Value::Bool(true));
    }

    // KV offload
    let offload_kqv = has_nvidia || is_apple_silicon;
    set_nested(&mut base, &["context", "offload_kqv"], serde_yaml::Value::Bool(offload_kqv));

    // Unified memory mode
    set_nested(&mut base, &["server", "unified_memory_mode"], serde_yaml::Value::Bool(is_unified_memory));

    // mmap (beneficial everywhere, especially for unified memory)
    set_nested(&mut base, &["model", "use_mmap"], serde_yaml::Value::Bool(true));

    // Thread count: use physical cores, cap for efficiency
    let threads = if is_apple_silicon {
        // On Apple Silicon, fewer threads is often better (efficiency cores hurt)
        (info.physical_cores / 2).max(2).min(8) as i32
    } else {
        info.physical_cores.min(16) as i32
    };
    set_nested(&mut base, &["context", "threads"], serde_yaml::Value::Number(threads.into()));
    set_nested(&mut base, &["context", "threads_batch"], serde_yaml::Value::Number(threads.into()));

    // --- Context size ---
    let n_slots = params.n_slots.unwrap_or(4);
    set_nested(&mut base, &["context", "seq_max"], serde_yaml::Value::Number(n_slots.into()));

    let gpu_vram_bytes = info.gpus.iter().find_map(|g| {
        g.memory.as_deref().and_then(helpers::parse_gpu_memory)
    });

    let available_mem = helpers::available_mem_for_ctx(
        info.memory_bytes,
        gpu_vram_bytes,
        model_size_gb,
        is_unified_memory,
    );

    let final_ctx = params
        .ctx
        .unwrap_or_else(|| helpers::recommend_ctx(available_mem, model_size_gb, n_slots));
    set_nested(&mut base, &["context", "size"], serde_yaml::Value::Number((final_ctx as i64).into()));

    // --- Fit section ---
    if params.enable_fit {
        set_nested(&mut base, &["fit", "enabled"], serde_yaml::Value::Bool(true));
        set_nested(&mut base, &["fit", "min_ctx"], serde_yaml::Value::Number(2048.into()));

        if !params.fit_target_mib.is_empty() {
            let targets: Vec<serde_yaml::Value> = params.fit_target_mib
                .iter()
                .map(|&m| serde_yaml::Value::Number((m as i64).into()))
                .collect();
            set_nested(&mut base, &["fit", "target_mib"], serde_yaml::Value::Sequence(targets));
        }
    }

    // Write to disk
    let dest_dir = paths::configs_dir();
    fs::create_dir_all(&dest_dir)?;
    let dest_file = dest_dir.join(format!("{}.yml", params.name));
    if dest_file.exists() && !params.overwrite {
        return Err(anyhow!("Config '{}' already exists", params.name));
    }

    let final_content = serde_yaml::to_string(&base)?;
    fs::write(&dest_file, &final_content)?;

    Ok(ConfigGenResult {
        path: dest_file.to_string_lossy().to_string(),
        model_path: model_display,
        ctx: final_ctx,
        mmproj_path: mmproj_path.as_deref().map(helpers::shorten_path),
    })
}

/// Set a value at a nested path like ["model", "path"].
fn set_nested(root: &mut serde_yaml::Value, keys: &[&str], value: serde_yaml::Value) {
    let mut current = root;
    for (i, key) in keys.iter().enumerate() {
        let yaml_key = serde_yaml::Value::String(key.to_string());
        if i == keys.len() - 1 {
            if let serde_yaml::Value::Mapping(map) = current {
                map.insert(yaml_key, value);
                return;
            }
        } else {
            if let serde_yaml::Value::Mapping(map) = current {
                if !map.contains_key(&yaml_key) {
                    map.insert(yaml_key.clone(), serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
                }
                current = map.get_mut(&yaml_key).unwrap();
            } else {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_nested_creates_intermediate_maps() {
        let mut root = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        set_nested(&mut root, &["server", "host"], serde_yaml::Value::String("0.0.0.0".to_string()));
        let host = root.get("server").unwrap().get("host").unwrap();
        assert_eq!(host.as_str(), Some("0.0.0.0"));
    }
}
