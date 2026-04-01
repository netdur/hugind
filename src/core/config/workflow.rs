use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowConfig {
    pub version: i32,
    pub name: String,

    /// V1: sequential steps
    #[serde(default)]
    pub steps: Vec<WorkflowStep>,

    /// V2: task DAG with dependencies
    #[serde(default)]
    pub tasks: Vec<WorkflowTask>,

    /// V2: named backend mappings (e.g. fast: gemma-4b, smart: qwen-32b)
    #[serde(default)]
    pub backends: HashMap<String, String>,
}

/// V1: simple sequential step
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowStep {
    pub name: String,
    pub agent: String,
}

/// V2: task with dependencies and optional backend override
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowTask {
    pub title: String,
    pub agent: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "depends_on")]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub backend: Option<String>,
}

impl WorkflowConfig {
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read workflow file at {:?}", path))?;

        let config: WorkflowConfig =
            serde_yaml::from_str(&content).with_context(|| "Failed to parse workflow YAML")?;

        Ok(config)
    }

    /// Returns true if this is a v2 workflow (uses tasks instead of steps).
    pub fn is_v2(&self) -> bool {
        self.version >= 2 && !self.tasks.is_empty()
    }
}
