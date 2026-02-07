use anyhow::Result;
use std::path::Path;
use std::fs;
use inquire::{Select, Text, Confirm};
use crate::shared::paths;
use crate::core::sys::{SystemInspector, SystemInfo};

pub fn init(name: String, model_override: Option<String>) -> Result<()> {
    
    println!("Probing hardware... (this may take a moment)");
    let info = SystemInspector::inspect();
    let recommended_preset = SystemInspector::recommend_preset(&info);
    
    print_hardware_summary(&info, recommended_preset);

    
    let presets = vec!["metal_unified", "cuda_dedicated", "cpu_only"];
    let default_idx = presets.iter().position(|&p| p == recommended_preset).unwrap_or(0);
    
    let chosen_preset = Select::new("Choose a hardware preset to apply", presets.clone())
        .with_starting_cursor(default_idx)
        .prompt()?;

    
    let base_content = include_str!("../resources/config.yml");
    let preset_content = match chosen_preset {
        "metal_unified" => include_str!("../resources/metal_unified.yml"),
        "cuda_dedicated" => include_str!("../resources/cuda_dedicated.yml"),
        "cpu_only" => include_str!("../resources/cpu_only.yml"),
        _ => "",
    };

    
    let (model_path, model_size_gb) = if let Some(m) = model_override {
        
        let size = if let Ok(meta) = fs::metadata(&m) {
            meta.len() as f64 / 1_073_741_824.0
        } else {
            0.0
        };
        (m, size)
    } else {
        use crate::core::model::registry::RepoManager;
        let repos = RepoManager::list_repos().unwrap_or_default();
        
        if repos.is_empty() {
             let path = Text::new("No repositories found in data home. Enter absolute path to .gguf file:")
                .with_help_message("Could not find any models in data directory.")
                .prompt()?;
             let size = if let Ok(meta) = fs::metadata(&path) {
                meta.len() as f64 / 1_073_741_824.0
             } else {
                0.0
             };
             (path, size)
        } else {
             
             let repo_options: Vec<String> = repos.iter().map(|r| r.full_name()).collect();
             let repo_selection = Select::new("Select a Model Repository", repo_options)
                .with_page_size(10)
                .prompt()?;
             
             let repo_idx = repos.iter().position(|r| r.full_name() == repo_selection).unwrap();
             let selected_repo = &repos[repo_idx];

             
             let files = RepoManager::list_repo_files(selected_repo)?;
             let gguf_files: Vec<&crate::core::model::registry::ModelFile> = files.iter()
                .filter(|f| f.name.ends_with(".gguf") && !f.name.starts_with("mmproj"))
                .collect();

             if gguf_files.is_empty() {
                 anyhow::bail!("No .gguf model files found in repository {}", repo_selection);
             }

             let file_options: Vec<String> = gguf_files.iter()
                .map(|f| f.name.clone())
                .collect();
             
             let file_selection = Select::new("Select the Model File", file_options)
                .with_page_size(10)
                .prompt()?;
             
             let file_idx = gguf_files.iter().position(|f| f.name == file_selection).unwrap();
             let selected_file = gguf_files[file_idx];
             
             (selected_file.path.to_string_lossy().to_string(), selected_file.size_gb())
        }
    };

    
    let mmproj_path = detect_sibling(&model_path, &["mmproj", "projector", "vision"]);
    if let Some(ref mm) = mmproj_path {
        println!("✨ Auto-detected Vision Projector: {}", Path::new(mm).file_name().unwrap_or_default().to_string_lossy());
    }

    
    let chat_formats = vec!["auto", "chatml", "chatmlThinking", "qwen3", "gemma", "alpaca", "harmony"];
    let detected_format = detect_chat_format(&model_path);
    let fmt_idx = chat_formats.iter().position(|&f| f == detected_format).unwrap_or(0);
    
    let chosen_format = Select::new("Select Chat Format Template", chat_formats)
        .with_starting_cursor(fmt_idx)
        .prompt()?;

    
    println!("\n🧠 Memory Analysis:");
    let sys_mem_gb = info.memory_bytes as f64 / 1_073_741_824.0;
    println!("  System RAM: {:.1} GB", sys_mem_gb);
    println!("  Model Size: {:.1} GB", model_size_gb);
    
    
    
    
    
    let available_for_ctx = (sys_mem_gb - model_size_gb - 2.0).max(0.5); 
    
    let est_tokens = (available_for_ctx * 10.0 * 1024.0) as u64;
    println!("  Est. Max Context: ~{} tokens", est_tokens);

    let ctx_options = vec![2048, 4096, 8192, 16384, 32768, 65536];
    
    let recommended_ctx = ctx_options.iter()
        .filter(|&&c| c as u64 <= est_tokens)
        .last()
        .copied()
        .unwrap_or(2048);

    let ctx_options_display: Vec<String> = ctx_options.iter().map(|&c| {
        if c == recommended_ctx {
            format!("{} (Recommended)", c)
        } else {
            c.to_string()
        }
    }).collect();

    let ctx_idx = ctx_options.iter().position(|&c| c == recommended_ctx).unwrap_or(1);
    
    let final_ctx_str = Select::new("Select Context Size (Ctx)", ctx_options_display)
        .with_starting_cursor(ctx_idx)
        .prompt()?;
    
    let final_ctx = final_ctx_str.split_whitespace().next().unwrap().parse::<u64>()?;

    
    
    
    let library_path = ""; 

    
    let mut final_content = base_content.to_string();
    
    
    for line in preset_content.lines() {
        if let Some((k, v)) = parse_yaml_line(line) {
            final_content = replace_value(&final_content, &k, &v);
        }
    }

    final_content = replace_value(&final_content, "path", &format!("\"{}\"", shorten_path(&model_path)));
    
    if let Some(mm) = &mmproj_path {
        final_content = replace_value(&final_content, "mmproj_path", &format!("\"{}\"", shorten_path(mm)));
        final_content = replace_value(&final_content, "batch_size", "8192");
    }

    if !library_path.is_empty() {
        final_content = replace_value(&final_content, "library_path", &format!("\"{}\"", library_path));
    }

    final_content = replace_value(&final_content, "format", chosen_format);
    final_content = replace_value(&final_content, "size", &final_ctx.to_string());

    
    let dest_dir = paths::configs_dir();
    fs::create_dir_all(&dest_dir)?;
    let dest_file = dest_dir.join(format!("{}.yml", name));
    
    if dest_file.exists() {
        if !Confirm::new(&format!("Config \"{}\" exists. Overwrite?", name))
            .with_default(false)
            .prompt()? 
        {
            return Ok(());
        }
    }

    fs::write(&dest_file, final_content)?;
    
    println!("\n✔ Config written to {:?}", dest_file);
    println!("  • Preset: {}", chosen_preset);
    println!("  • Model: {}", shorten_path(&model_path));
    println!("  • Context: {}", final_ctx);

    Ok(())
}

fn print_hardware_summary(info: &SystemInfo, recommendation: &str) {
    println!("System probe complete:");
    println!("  CPU: {} ({}c/{}t)", info.cpu_model, info.physical_cores, info.logical_cores);
    println!("  Memory: {:.1} GB", info.memory_bytes as f64 / 1_073_741_824.0);
    
    println!("Recommended preset: {}", recommendation);
}

fn detect_sibling(main_path: &str, keywords: &[&str]) -> Option<String> {
    let path = Path::new(main_path);
    if !path.exists() { return None; }
    
    let parent = path.parent()?;
    let main_name = path.file_name()?.to_str()?.to_lowercase();
    
    if let Ok(entries) = fs::read_dir(parent) {
        for entry in entries.filter_map(Result::ok) {
            let p = entry.path();
            if p.is_file() {
                if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                    let name_lower = name.to_lowercase();
                    if name_lower != main_name && name_lower.ends_with(".gguf") {
                        if keywords.iter().any(|k| name_lower.contains(k)) {
                            return Some(p.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

fn detect_chat_format(path: &str) -> &'static str {
    let lower = path.to_lowercase();
    if lower.contains("gemma") { return "gemma"; }
    if lower.contains("llama-3") { return "alpaca"; }
    if lower.contains("qwen") || lower.contains("yi") { return "chatml"; }
    "none"
}

fn shorten_path(path: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
         if path.starts_with(home_str.as_ref()) {
             return path.replacen(home_str.as_ref(), "~", 1);
         }
    }
    path.to_string()
}


fn replace_value(content: &str, key: &str, new_value: &str) -> String {
    let mut output = String::new();
    let key_pat = format!("{}:", key);
    
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(&key_pat) {
            
            
            let indent = &line[0..line.len() - trimmed.len()];
            
            let parts: Vec<&str> = trimmed.splitn(2, ':').collect();
            if parts.len() == 2 {
                let rest = parts[1];
                let comment_idx = rest.find('#');
                let comment = if let Some(idx) = comment_idx {
                    &rest[idx..]
                } else {
                    ""
                };
                
                output.push_str(&format!("{}{}: {}{}\n", indent, key, new_value, if comment.is_empty() { String::new() } else { format!("  {}", comment) }));
                continue;
            }
        }
        output.push_str(line);
        output.push('\n');
    }
    output
}

fn parse_yaml_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.starts_with('#') || line.is_empty() { return None; }
    let parts: Vec<&str> = line.splitn(2, ':').collect();
    if parts.len() == 2 {
        let key = parts[0].trim().to_string();
        let val_part = parts[1];
        let val = val_part.split('#').next()?.trim().to_string();
        if !val.is_empty() {
            return Some((key, val));
        }
    }
    None
}
