use crate::shared::paths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct GlobalSettings(pub HashMap<String, String>);

impl GlobalSettings {
    pub fn load() -> Result<Self> {
        let path = paths::data_home().join("settings.yml");
        if !path.exists() {
            return Ok(GlobalSettings::default());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read global settings: {:?}", path))?;
        serde_yaml::from_str(&content).with_context(|| "Failed to parse settings.yml")
    }

    pub fn save(&self) -> Result<()> {
        let path = paths::data_home().join("settings.yml");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_yaml::to_string(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.0.insert(key.to_string(), value.to_string());
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.0.get(key)
    }
}
