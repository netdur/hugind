use serde::Deserialize;
use std::path::Path;
use anyhow::{Context, Result};

#[derive(Debug, Deserialize, Default, Clone)]
pub struct NetPermissions {
    #[serde(default)]
    pub allow: bool,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub allowed_ips: Vec<String>,
    #[serde(default)]
    pub block_private_networks: bool,
    pub max_response_bytes: Option<String>,
    pub timeout: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct FileSystemPermission {
    #[serde(default)]
    pub allow: bool,
    #[serde(default = "default_true")]
    pub read: bool,
    #[serde(default = "default_true")]
    pub write: bool,
    #[serde(default)]
    pub create: bool,
    #[serde(default)]
    pub delete: bool,
    #[serde(default)]
    pub allow_outside_agent_root: bool,
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    #[serde(default)]
    pub denied_paths: Vec<String>,
    #[serde(default)]
    pub follow_symlinks: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct ShellPermission {
    #[serde(default)]
    pub allow: bool,
    pub whitelist: Option<Vec<String>>,
    pub blacklist: Option<Vec<String>>,
    pub timeout: Option<String>,
    pub max_output: Option<String>,
    #[serde(default)]
    pub env_clear: bool,
    pub working_dir: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct Permissions {
    #[serde(default)]
    pub network: Option<NetPermissions>,
    #[serde(default)]
    pub filesystem: Option<FileSystemPermission>,
    #[serde(default)]
    pub shell: Option<ShellPermission>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    Stateless,
    Fresh,
    Resume,
}

impl Default for SessionMode {
    fn default() -> Self {
        Self::Stateless
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeSession {
    pub mode: SessionMode,
    pub id: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Mount {
    pub host: String,
    pub guest: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WasmResources {
    pub memory: Option<String>,
    pub cpu: Option<String>,
    pub timeout: Option<String>,
    pub max_output: Option<String>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFsMode {
    WasiMounts,
    HostFilesystem,
    Both,
}

impl Default for RuntimeFsMode {
    fn default() -> Self {
        Self::Both
    }
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct WasmConfig {
    #[serde(default)]
    pub runtime_fs_mode: RuntimeFsMode,
    pub mounts: Option<Vec<Mount>>,
    pub resources: Option<WasmResources>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AgentConfig {
    pub name: String,
    pub version: String,
    pub hugind_version: Option<String>,
    pub entry_point: String,
    #[serde(default)]
    pub wasm: Option<WasmConfig>,
    
    pub backend: Option<serde_yaml::Value>,
    #[serde(default)]
    pub permissions: Option<Permissions>,
    pub dependencies: Option<serde_yaml::Value>,
    pub env: Option<Vec<serde_yaml::Value>>,
    #[serde(skip)]
    pub runtime_session: Option<RuntimeSession>,
}

impl AgentConfig {
    pub fn load_from_dir(path: &Path) -> Result<Self> {
        let config_path = path.join("agent.yaml");
        let content = std::fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read agent.yaml at {:?}", config_path))?;
        
        let config: AgentConfig = serde_yaml::from_str(&content)
            .with_context(|| "Failed to parse agent.yaml")?;
            
        Ok(config)
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            version: "0.0.0".to_string(),
            hugind_version: None,
            entry_point: "main.js".to_string(),
            wasm: None,
            backend: None,
            permissions: Some(Permissions::default()),
            dependencies: None,
            env: None,
            runtime_session: None,
        }
    }
}
