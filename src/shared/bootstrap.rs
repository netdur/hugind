use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

use crate::shared::paths;

pub fn ensure_user_home() -> Result<()> {
    let config_home = paths::config_home();
    let data_home = paths::data_home();

    fs::create_dir_all(&config_home)
        .with_context(|| format!("Failed to create {}", config_home.display()))?;
    fs::create_dir_all(&data_home)
        .with_context(|| format!("Failed to create {}", data_home.display()))?;

    ensure_dir(paths::configs_dir())?;
    ensure_dir(paths::agents_dir())?;
    ensure_dir(paths::sessions_dir())?;

    let models_dir = paths::models_dir();
    let models_is_new = !models_dir.exists();
    ensure_dir(models_dir)?;

    // Auto-migrate models from old layout (~/.hugind/{user}/{repo}/)
    // to new layout (~/.hugind/models/{user}/{repo}/) on first run.
    if models_is_new {
        if let Ok(n) = crate::core::model::migrate::migrate_models_to_models_dir() {
            if n > 0 {
                eprintln!("Migrated {} model repo(s) to ~/.hugind/models/", n);
            }
        }
    }

    Ok(())
}

fn ensure_dir(path: PathBuf) -> Result<()> {
    fs::create_dir_all(&path).with_context(|| format!("Failed to create {}", path.display()))?;
    Ok(())
}

// Default configs and settings are created by specific commands, not at startup.
