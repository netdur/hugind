use crate::shared::paths;
use anyhow::Result;
use std::fs;

/// Migrate models from the old layout (`~/.hugind/{user}/{repo}/`)
/// to the new dedicated directory (`~/.hugind/models/{user}/{repo}/`).
/// Also updates model paths in config files under `~/.hugind/configs/`.
///
/// Returns the number of repos migrated.
pub fn migrate_models_to_models_dir() -> Result<u32> {
    let root = paths::data_home();
    let models_dir = paths::models_dir();

    if !root.exists() {
        return Ok(0);
    }

    // Known non-model directories that live directly under data_home
    let skip = [
        "models", "configs", "agents", "sessions", "logs", "chat", "chats",
    ];

    let mut migrated = 0u32;
    let mut moved_paths: Vec<(String, String)> = Vec::new();

    for user_entry in fs::read_dir(&root)? {
        let user_entry = user_entry?;
        let user_name = user_entry.file_name().to_string_lossy().to_string();

        if !user_entry.path().is_dir() || user_name.starts_with('.') || skip.contains(&user_name.as_str()) {
            continue;
        }

        // Check if any sub-entry is a directory containing .gguf files
        let mut has_model_repos = false;
        for repo_entry in fs::read_dir(user_entry.path())? {
            let repo_entry = repo_entry?;
            if !repo_entry.path().is_dir() {
                continue;
            }
            if dir_has_gguf(&repo_entry.path())? {
                has_model_repos = true;
                break;
            }
        }

        if !has_model_repos {
            continue;
        }

        // Move user/{repo} dirs that contain .gguf files
        for repo_entry in fs::read_dir(user_entry.path())? {
            let repo_entry = repo_entry?;
            if !repo_entry.path().is_dir() {
                continue;
            }
            if !dir_has_gguf(&repo_entry.path())? {
                continue;
            }

            let repo_name = repo_entry.file_name().to_string_lossy().to_string();
            let dest_user_dir = models_dir.join(&user_name);
            let dest_repo_dir = dest_user_dir.join(&repo_name);

            let old_path = repo_entry.path();

            // Track old→new path prefixes for config rewriting regardless
            record_path_mapping(&old_path, &dest_repo_dir, &mut moved_paths);

            if dest_repo_dir.exists() {
                // Already moved — skip file move but still record for config rewriting
                continue;
            }

            fs::create_dir_all(&dest_user_dir)?;
            fs::rename(&old_path, &dest_repo_dir)?;
            migrated += 1;
        }

        // Clean up empty user dir in old location
        if let Ok(entries) = fs::read_dir(user_entry.path()) {
            if entries.count() == 0 {
                let _ = fs::remove_dir(user_entry.path());
            }
        }
    }

    // Always rewrite config paths — covers cases where files were moved
    // previously but configs weren't updated
    let config_updated = rewrite_config_model_paths(&root, &models_dir)?;

    // Also apply any path mappings we collected from this run
    if !moved_paths.is_empty() {
        let _ = update_config_paths(&moved_paths);
    }

    if config_updated > 0 {
        eprintln!("Updated model paths in {} config file(s)", config_updated);
    }

    Ok(migrated)
}

fn record_path_mapping(old_path: &std::path::Path, new_path: &std::path::Path, moved_paths: &mut Vec<(String, String)>) {
    let old_abs = old_path.to_string_lossy().to_string();
    let new_abs = new_path.to_string_lossy().to_string();
    moved_paths.push((old_abs.clone(), new_abs.clone()));

    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        let old_tilde = old_abs.replacen(home_str.as_ref(), "~", 1);
        let new_tilde = new_abs.replacen(home_str.as_ref(), "~", 1);
        if old_tilde != old_abs {
            moved_paths.push((old_tilde, new_tilde));
        }
    }
}

/// Rewrite config files that reference the old data_home layout to use models_dir.
/// Works by pattern: replaces `{data_home}/{user}/` with `{models_dir}/{user}/`
/// for any path that looks like it points to a model file.
fn rewrite_config_model_paths(data_home: &std::path::Path, models_dir: &std::path::Path) -> Result<u32> {
    let configs_dir = paths::configs_dir();
    if !configs_dir.exists() {
        return Ok(0);
    }

    let data_home_abs = data_home.to_string_lossy().to_string();
    let models_dir_abs = models_dir.to_string_lossy().to_string();

    // Build tilde variants
    let (data_home_tilde, models_dir_tilde) = if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        (
            data_home_abs.replacen(home_str.as_ref(), "~", 1),
            models_dir_abs.replacen(home_str.as_ref(), "~", 1),
        )
    } else {
        (data_home_abs.clone(), models_dir_abs.clone())
    };

    // Skip list: these dirs under data_home are NOT model dirs
    let skip = ["models/", "configs/", "agents/", "sessions/", "logs/", "chat/", "chats/", "settings.yml"];

    let mut updated = 0u32;

    for entry in fs::read_dir(&configs_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "yml" && ext != "yaml" {
            continue;
        }

        let content = fs::read_to_string(&path)?;
        let mut new_content = content.clone();

        // For each line that contains a data_home path, check if it's a model path
        // and rewrite it to use models_dir
        for (old_base, new_base) in [
            (&data_home_abs, &models_dir_abs),
            (&data_home_tilde, &models_dir_tilde),
        ] {
            if !new_content.contains(old_base) {
                continue;
            }
            // Don't replace paths that point to known non-model dirs
            let should_replace = |line: &str| -> bool {
                if !line.contains(old_base) {
                    return false;
                }
                // Check it's not pointing to a known subdir
                for s in &skip {
                    let pattern = format!("{}/{}", old_base, s);
                    if line.contains(&pattern) {
                        return false;
                    }
                }
                // Must contain .gguf to be a model path
                line.contains(".gguf")
            };

            let lines: Vec<&str> = new_content.lines().collect();
            let replaced: Vec<String> = lines
                .iter()
                .map(|line| {
                    if should_replace(line) {
                        line.replace(old_base, new_base)
                    } else {
                        line.to_string()
                    }
                })
                .collect();
            new_content = replaced.join("\n");
            // Preserve trailing newline if original had one
            if content.ends_with('\n') && !new_content.ends_with('\n') {
                new_content.push('\n');
            }
        }

        if new_content != content {
            fs::write(&path, &new_content)?;
            updated += 1;
        }
    }

    Ok(updated)
}

/// Scan config files and replace old model paths with new ones.
fn update_config_paths(moved_paths: &[(String, String)]) -> Result<u32> {
    let configs_dir = paths::configs_dir();
    if !configs_dir.exists() {
        return Ok(0);
    }

    let mut updated = 0u32;

    for entry in fs::read_dir(&configs_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "yml" && ext != "yaml" {
            continue;
        }

        let content = fs::read_to_string(&path)?;
        let mut new_content = content.clone();

        for (old_prefix, new_prefix) in moved_paths {
            if new_content.contains(old_prefix) {
                new_content = new_content.replace(old_prefix, new_prefix);
            }
        }

        if new_content != content {
            fs::write(&path, &new_content)?;
            updated += 1;
        }
    }

    Ok(updated)
}

fn dir_has_gguf(dir: &std::path::Path) -> Result<bool> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext.eq_ignore_ascii_case("gguf") {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
