use crate::core::config::helpers;
use crate::core::config::settings::GlobalSettings;
use crate::core::sys::SystemInspector;
use crate::shared::paths;
use anyhow::Result;
use std::fs;
use std::path::Path;

const SENSITIVE_KEYS: &[&str] = &["hf_token"];

pub fn list() -> Result<()> {
    let items = helpers::list_config_names()?;

    if items.is_empty() {
        println!("No configs found.");
        return Ok(());
    }

    println!("Saved Configs:");
    for (name, _path) in items {
        println!("- {}", name);
    }
    Ok(())
}

pub fn validate(path: String) -> Result<()> {
    let config_path = Path::new(&path);
    if !config_path.exists() {
        anyhow::bail!("Config file not found: {}", path);
    }

    match crate::core::config::loader::ConfigLoader::load_server_config(config_path) {
        Ok(_) => {
            println!("Configuration is valid.");
            Ok(())
        }
        Err(e) => {
            println!("Configuration Invalid: {:#}", e);
            Err(e)
        }
    }
}

pub fn info() -> Result<()> {
    let info = SystemInspector::inspect();
    println!("System Information");
    println!("------------------");
    println!("OS: {}", info.os);
    println!("Arch: {}", info.arch);
    println!("CPU: {}", info.cpu_model);
    println!(
        "Cores: {} physical / {} logical",
        info.physical_cores, info.logical_cores
    );
    println!(
        "Memory: {:.1} GB",
        info.memory_bytes as f64 / 1_073_741_824.0
    );
    println!(
        "Disk: {:.1} GB total / {:.1} GB free",
        info.disk_total_bytes as f64 / 1_073_741_824.0,
        info.disk_available_bytes as f64 / 1_073_741_824.0
    );

    if info.gpus.is_empty() {
        println!("GPUs: None detected");
    } else {
        println!("GPUs:");
        for gpu in &info.gpus {
            let mem_str = gpu.memory.as_deref().unwrap_or("Unknown VRAM");
            println!("  - {} ({})", gpu.name, mem_str);
        }
    }

    Ok(())
}

pub fn remove(name: String) -> Result<()> {
    let path_to_remove = helpers::find_config_path(&name);

    if let Some(p) = path_to_remove {
        if inquire::Confirm::new(&format!("Delete config \"{}\"?", name))
            .with_default(false)
            .prompt()?
        {
            fs::remove_file(&p)?;
            println!("Deleted.");
        } else {
            println!("Cancelled.");
        }
    } else {
        println!("Config \"{}\" not found.", name);
    }
    Ok(())
}

pub fn defaults(hf_token: Option<String>, set_pairs: Vec<String>) -> Result<()> {
    ensure_settings_file()?;

    let mut settings = GlobalSettings::load()?;
    let mut changed = false;

    if let Some(t) = hf_token {
        settings.set("hf_token", &t);
        println!("Global Hugging Face Token updated.");
        changed = true;
    }

    for pair in &set_pairs {
        if let Some((key, value)) = pair.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            settings.set(key, value);
            println!("Set {} = {}", key, if SENSITIVE_KEYS.contains(&key) { helpers::mask_sensitive_value(value) } else { value.to_string() });
            changed = true;
        } else {
            anyhow::bail!("Invalid format '{}', expected key=value", pair);
        }
    }

    if changed {
        settings.save()?;
        return Ok(());
    }

    // Display mode
    println!(
        "\nGlobal Settings ({:?}):",
        paths::data_home().join("settings.yml")
    );
    println!("----------------------------------------");
    if settings.0.is_empty() {
        println!("No defaults set.");
    } else {
        for (k, v) in &settings.0 {
            let display = if SENSITIVE_KEYS.contains(&k.as_str()) {
                helpers::mask_sensitive_value(v)
            } else {
                v.clone()
            };
            println!("{}: {}", k, display);
        }
    }
    println!("----------------------------------------");
    println!("\nUsage:");
    println!("  hugind config defaults --hf-token hf_xxxxxx");
    println!("  hugind config defaults --set key=value");
    Ok(())
}

fn ensure_settings_file() -> Result<()> {
    let path = paths::data_home().join("settings.yml");
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = include_str!("../../assets/settings.yml");
    fs::write(&path, content)?;
    Ok(())
}
