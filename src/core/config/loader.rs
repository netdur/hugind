use crate::core::config::server::*;
use crate::shared::paths;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

pub struct ConfigLoader;

impl ConfigLoader {
    pub fn load_server_config(path: &Path) -> Result<ServerConfig> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {:?}", path))?;

        let raw: RawConfigFile =
            serde_yaml::from_str(&content).with_context(|| "Failed to parse YAML")?;

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let host = raw.server.host;
        let port = raw.server.port;
        let api_key = trim_non_empty(raw.server.api_key);
        let max_slots = raw.server.max_slots.unwrap_or(raw.context.n_seq_max);

        let system_prompt_file =
            trim_non_empty(raw.server.system_prompt_file).map(|p| resolve_path_relative(&p, path));
        let system_prompt = if let Some(p) = &system_prompt_file {
            fs::read_to_string(p)
                .with_context(|| format!("Failed to read system_prompt_file: {:?}", p))?
        } else {
            raw.server.system_prompt
        };

        let embeddings_enabled = raw
            .server
            .embeddings
            .as_ref()
            .and_then(Boolish::as_bool)
            .unwrap_or(raw.context.embeddings);
        let unified_memory_mode = raw
            .server
            .unified_memory_mode
            .as_ref()
            .and_then(Boolish::as_bool)
            .unwrap_or(false);
        let verbose = raw
            .server
            .verbose
            .as_ref()
            .and_then(Boolish::as_bool)
            .unwrap_or(false);

        let session_home = trim_non_empty(raw.server.session_home)
            .map(|s| resolve_path_relative(&s, path))
            .unwrap_or_else(paths::sessions_dir);

        let model_name = trim_non_empty(raw.model.name);
        let model_path_str = raw.model.path.trim().to_string();
        let model_path = resolve_path_relative(&model_path_str, path);
        if !model_path_str.is_empty() && model_path_str != "@PLACEHOLDER" && !model_path.exists() {
            anyhow::bail!("Model file not found at: {:?}", model_path);
        }

        let mmproj_path =
            trim_non_empty(raw.model.mmproj_path).map(|p| resolve_path_relative(&p, path));

        let mut model_params = raw.model.params;
        let mut context_params = raw.context;
        let multimodal_params = raw.multimodal;
        let mut sampler_params = raw.sampling;
        let enable_thinking_default = raw.server.enable_thinking_default
            .as_ref()
            .and_then(Boolish::as_bool)
            .unwrap_or(false);
        let thinking_budget_tokens = raw.server.thinking_budget_tokens;
        let mut lora_params = raw.lora;
        let fit_params = raw.fit;
        let quantize_params = raw.quantize;
        let advanced_params = raw.advanced;

        // Preserve legacy `server.max_slots` behavior while supporting `context.seq_max`.
        context_params.n_seq_max = max_slots;
        context_params.embeddings = embeddings_enabled;

        // For vision models, keep n_batch large enough for stable image token eval.
        if mmproj_path.is_some() && context_params.n_batch < 8192 {
            println!("Vision model detected with low batch size. Auto-increasing to 8192.");
            context_params.n_batch = 8192;
        }

        // Keep explicit flash-attn enum aligned when alias is enabled.
        if context_params.flash_attention && context_params.flash_attn_type == FlashAttnType::Auto {
            context_params.flash_attn_type = FlashAttnType::On;
        }

        // Empty grammar should behave like "disabled".
        sampler_params.grammar = sampler_params.grammar.trim().to_string();

        // Resolve relative LoRA/control-vector paths against config location.
        lora_params.adapters = lora_params
            .adapters
            .into_iter()
            .map(|p| resolve_path_relative(&p.to_string_lossy(), path))
            .collect();
        for adapter in &mut lora_params.scaled_adapters {
            adapter.path = resolve_path_relative(&adapter.path.to_string_lossy(), path);
        }
        for vector in &mut lora_params.control_vectors {
            vector.path = resolve_path_relative(&vector.path.to_string_lossy(), path);
        }

        // Ensure main_gpu is valid if no explicit devices were chosen.
        if model_params.devices.is_empty() && model_params.main_gpu < 0 {
            model_params.main_gpu = 0;
        }

        Ok(ServerConfig {
            name,
            host,
            port,
            api_key,
            max_slots,
            system_prompt,
            system_prompt_file,
            embeddings_enabled,
            session_home,
            unified_memory_mode,
            verbose,
            model_path,
            mmproj_path,
            model_name,
            model_params,
            context_params,
            multimodal_params,
            sampler_params,
            enable_thinking_default,
            thinking_budget_tokens,
            lora_params,
            fit_params,
            quantize_params,
            advanced_params,
        })
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawConfigFile {
    server: RawServerSection,
    model: RawModelSection,
    context: ContextParams,
    multimodal: MultimodalParams,
    sampling: SamplerParams,
    lora: LoraParams,
    fit: FitParams,
    quantize: QuantizeParams,
    advanced: AdvancedParams,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct RawServerSection {
    host: String,
    port: u16,
    api_key: Option<String>,
    max_slots: Option<u32>,
    system_prompt: String,
    system_prompt_file: Option<String>,
    embeddings: Option<Boolish>,
    session_home: Option<String>,
    unified_memory_mode: Option<Boolish>,
    verbose: Option<Boolish>,
    enable_thinking_default: Option<Boolish>,
    thinking_budget_tokens: Option<u32>,
}

impl Default for RawServerSection {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8080,
            api_key: None,
            max_slots: None,
            system_prompt: "You are a helpful assistant.".to_string(),
            system_prompt_file: None,
            embeddings: None,
            session_home: None,
            unified_memory_mode: None,
            verbose: None,
            enable_thinking_default: None,
            thinking_budget_tokens: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct RawModelSection {
    path: String,
    mmproj_path: Option<String>,
    name: Option<String>,
    #[serde(flatten)]
    params: ModelParams,
}

impl Default for RawModelSection {
    fn default() -> Self {
        Self {
            path: String::new(),
            mmproj_path: None,
            name: None,
            params: ModelParams::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum Boolish {
    Bool(bool),
    String(String),
}

impl Boolish {
    fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            Self::String(s) => {
                let normalized = s.trim().to_ascii_lowercase();
                match normalized.as_str() {
                    "true" | "on" | "yes" | "enabled" | "1" => Some(true),
                    "false" | "off" | "no" | "disabled" | "0" => Some(false),
                    _ => {
                        eprintln!(
                            "Warning: unrecognized boolean value '{}', expected true/false/on/off/yes/no/enabled/disabled/1/0",
                            s
                        );
                        None
                    }
                }
            }
        }
    }
}

fn resolve_path_relative(raw_path: &str, config_path: &Path) -> PathBuf {
    if raw_path.is_empty() {
        return PathBuf::new();
    }

    let mut resolved = raw_path.to_string();
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

fn trim_non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::ConfigLoader;
    use std::fs;

    #[test]
    fn parses_realistic_template_shape() {
        let dir = tempfile::tempdir().expect("tempdir");
        let model_path = dir.path().join("model.gguf");
        fs::write(&model_path, b"not-a-real-model").expect("write model");

        let template = include_str!("../../resources/config.yml");
        let yaml = template
            .replace("@PLACEHOLDER", &model_path.to_string_lossy())
            .replace("mmproj_path: \"\"", "mmproj_path: \"proj.gguf\"");

        let config_path = dir.path().join("config.yml");
        fs::write(&config_path, yaml).expect("write yaml");

        let config = ConfigLoader::load_server_config(&config_path).expect("load config");
        assert_eq!(config.model_params.n_gpu_layers, 99);
        assert_eq!(config.context_params.n_ctx, 4096);
        assert_eq!(config.context_params.n_seq_max, 4);
        assert_eq!(config.sampler_params.top_k, 40);
        assert_eq!(config.multimodal_params.image_max_tokens, 0);
        assert!(!config.enable_thinking_default);
        assert_eq!(config.thinking_budget_tokens, None);
        assert!(config.advanced_params.warmup);
    }
}
