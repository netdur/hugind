use serde::{Deserialize, Serialize};
use std::path::Path;
use anyhow::{Result, Context};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowConfig {
    pub version: i32,
    pub name: String,
    pub steps: Vec<WorkflowStep>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowStep {
    pub name: String,
    pub agent: String,
}

impl WorkflowConfig {
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read workflow file at {:?}", path))?;
        
        let config: WorkflowConfig = serde_yaml::from_str(&content)
            .with_context(|| "Failed to parse workflow YAML")?;
            
        Ok(config)
    }
}
