use std::path::Path;
use std::fs;
use anyhow::Result;
use crate::shared::paths;
use crate::core::sys::SystemInspector;
use crate::core::config::settings::GlobalSettings;

pub fn list() -> Result<()> {
    let config_dir = paths::configs_dir();
    
    if !config_dir.exists() {
        println!("No configs found (directory does not exist: {:?}).", config_dir);
        return Ok(());
    }

    let mut found = false;
    println!("Saved Configs:");
    
    for entry in fs::read_dir(&config_dir)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if ext == "yml" || ext == "yaml" {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        println!("- {}", stem);
                        found = true;
                    }
                }
            }
        }
    }

    if !found {
        println!("No configs found.");
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
    println!("Cores: {} physical / {} logical", info.physical_cores, info.logical_cores);
    println!("Memory: {:.1} GB", info.memory_bytes as f64 / 1_073_741_824.0);
    println!("Disk: {:.1} GB total / {:.1} GB free", 
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

    let preset = SystemInspector::recommend_preset(&info);
    println!("\nRecommendation: {}", preset);
    Ok(())
}

pub fn remove(name: String) -> Result<()> {
    let config_dir = paths::configs_dir();
    let yml_path = config_dir.join(format!("{}.yml", name));
    let yaml_path = config_dir.join(format!("{}.yaml", name));
    
    let path_to_remove = if yml_path.exists() {
        Some(yml_path)
    } else if yaml_path.exists() {
        Some(yaml_path)
    } else {
        None
    };

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

pub fn defaults(lib: Option<String>, hf_token: Option<String>) -> Result<()> {
    ensure_settings_file()?;
    if lib.is_none() && hf_token.is_none() {
        let settings = GlobalSettings::load()?;
        println!("\nGlobal Settings ({:?}):", paths::data_home().join("settings.yml"));
        println!("----------------------------------------");
        if settings.0.is_empty() {
            println!("No defaults set.");
        } else {
            for (k, v) in &settings.0 {
                println!("{}: {}", k, v);
            }
        }
        println!("----------------------------------------");
        println!("\nUsage:");
        println!("  hugind config defaults --lib /path/to/libllama.dylib");
        println!("  hugind config defaults --hf-token hf_xxxxxx");
        return Ok(());
    }

    let mut settings = GlobalSettings::load()?;
    
    if let Some(l) = lib {
         if !Path::new(&l).exists() {
             println!("⚠️  Warning: File does not exist at {}", l);
         }
         settings.set("library_path", &l);
         println!("✅ Global Library Path updated.");
    }

    if let Some(t) = hf_token {
        settings.set("hf_token", &t);
        println!("✅ Global Hugging Face Token updated.");
    }

    settings.save()?;
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
