use serde::Deserialize;
use std::path::Path;
use anyhow::{Context, Result};

#[derive(Debug, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub version: String,
    pub entry_point: String,
    // We keep these flexible for now
    pub backend: Option<serde_yaml::Value>,
    pub permissions: Option<serde_yaml::Value>,
    pub dependencies: Option<serde_yaml::Value>,
    pub env: Option<Vec<serde_yaml::Value>>,
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
            entry_point: "main.js".to_string(),
            backend: None,
            permissions: None,
            dependencies: None,
            env: None,
        }
    }
}
