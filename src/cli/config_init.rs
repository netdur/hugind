use crate::core::config::generator::{self, ConfigGenParams};
use crate::core::config::helpers;
use crate::core::sys::SystemInspector;
use anyhow::Result;
use inquire::{Confirm, Select, Text};
use std::fs;

pub fn init(name: String, model_override: Option<String>) -> Result<()> {
    helpers::validate_config_name(&name)?;

    println!("Probing hardware...");
    let info = SystemInspector::inspect();

    let sys_mem_gb = info.memory_bytes as f64 / 1_073_741_824.0;
    let is_apple_silicon = cfg!(target_os = "macos") && std::env::consts::ARCH == "aarch64";
    let has_nvidia = info.gpus.iter().any(|g| g.name.to_lowercase().contains("nvidia"));
    let is_unified = is_apple_silicon;

    println!("  CPU: {} ({}c/{}t)", info.cpu_model, info.physical_cores, info.logical_cores);
    println!("  RAM: {:.1} GB", sys_mem_gb);
    if info.gpus.is_empty() {
        println!("  GPU: None detected (CPU-only mode)");
    } else {
        for gpu in &info.gpus {
            let mem_str = gpu.memory.as_deref().unwrap_or(if is_unified { "shared with RAM" } else { "unknown" });
            println!("  GPU: {} ({})", gpu.name, mem_str);
        }
    }
    if is_unified {
        println!("  Memory: Unified (model + KV cache share RAM)");
    }

    // --- Model selection ---
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
            let path =
                Text::new("No downloaded models found. Enter absolute path to .gguf file:")
                    .prompt()?;
            let size = if let Ok(meta) = fs::metadata(&path) {
                meta.len() as f64 / 1_073_741_824.0
            } else {
                0.0
            };
            (path, size)
        } else {
            let repo_options: Vec<String> = repos.iter().map(|r| r.full_name()).collect();
            let repo_selection = Select::new("Select a Model Repository:", repo_options)
                .with_page_size(10)
                .prompt()?;

            let repo_idx = repos
                .iter()
                .position(|r| r.full_name() == repo_selection)
                .unwrap();
            let selected_repo = &repos[repo_idx];

            let files = RepoManager::list_repo_files(selected_repo)?;
            let gguf_files: Vec<&crate::core::model::registry::ModelFile> = files
                .iter()
                .filter(|f| f.name.ends_with(".gguf") && !f.name.starts_with("mmproj"))
                .collect();

            if gguf_files.is_empty() {
                anyhow::bail!("No .gguf model files found in repository {}", repo_selection);
            }

            // Show file sizes in selection
            let file_options: Vec<String> = gguf_files
                .iter()
                .map(|f| format!("{} ({:.1} GB)", f.name, f.size_gb()))
                .collect();

            let file_selection = Select::new("Select the Model File:", file_options)
                .with_page_size(10)
                .prompt()?;

            let file_idx = gguf_files
                .iter()
                .position(|f| format!("{} ({:.1} GB)", f.name, f.size_gb()) == file_selection)
                .unwrap();
            let selected_file = gguf_files[file_idx];

            (
                selected_file.path.to_string_lossy().to_string(),
                selected_file.size_gb(),
            )
        }
    };

    let mmproj_path = helpers::detect_sibling(&model_path, &["mmproj", "projector", "vision"]);
    if let Some(ref mm) = mmproj_path {
        println!(
            "  Vision projector detected: {}",
            std::path::Path::new(mm)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        );
    }

    // --- Memory analysis ---
    println!("\nMemory budget:");
    println!("  Model file: {:.1} GB", model_size_gb);

    let gpu_vram_bytes = info.gpus.iter().find_map(|g| {
        g.memory.as_deref().and_then(helpers::parse_gpu_memory)
    });

    let available_mem = helpers::available_mem_for_ctx(
        info.memory_bytes,
        gpu_vram_bytes,
        model_size_gb,
        is_unified,
    );
    let available_mem_gb = available_mem as f64 / 1_073_741_824.0;

    if is_unified {
        println!("  Available for KV cache: ~{:.1} GB (RAM - model - 2 GB headroom)", available_mem_gb);
    } else if has_nvidia {
        if let Some(vram) = gpu_vram_bytes {
            let vram_gb = vram as f64 / 1_073_741_824.0;
            println!("  GPU VRAM: {:.1} GB", vram_gb);
            let vram_after = vram.saturating_sub((model_size_gb * 1_073_741_824.0) as u64);
            if vram_after > 512 * 1_048_576 {
                println!("  Available for KV cache: ~{:.1} GB (VRAM after model)", available_mem_gb);
            } else {
                println!("  Model fills most of VRAM, KV cache will use RAM");
                println!("  Available for KV cache: ~{:.1} GB", available_mem_gb);
            }
        }
    } else {
        println!("  Available for KV cache: ~{:.1} GB (RAM - model - 2 GB headroom)", available_mem_gb);
    }

    // --- Slots ---
    let n_slots = 4u32;

    // --- Fit or manual context ---
    println!("\nAuto-fit lets the engine determine the best context size at startup");
    println!("based on your available memory. You can skip this and choose manually.");
    let enable_fit = Confirm::new("Enable auto-fit?")
        .with_default(true)
        .prompt()?;

    let final_ctx = if enable_fit {
        // With fit enabled, use a generous default — the engine will shrink if needed
        let recommended = helpers::recommend_ctx(available_mem, model_size_gb, n_slots);
        println!("  Starting context: {} tokens (engine will adjust to fit)", recommended);
        Some(recommended)
    } else {
        let recommended = helpers::recommend_ctx(available_mem, model_size_gb, n_slots);
        println!("  Recommended context: {} tokens ({} slots)", recommended, n_slots);

        let options: Vec<u64> = vec![2048, 4096, 8192, 16384, 32768, 65536, 131072, 262144];
        let display: Vec<String> = options
            .iter()
            .map(|&c| {
                if c == recommended {
                    format!("{} (recommended)", c)
                } else if c > recommended * 2 {
                    format!("{} (may not fit)", c)
                } else {
                    c.to_string()
                }
            })
            .collect();

        let ctx_idx = options
            .iter()
            .position(|&c| c == recommended)
            .unwrap_or(1);

        let final_ctx_str = Select::new("Context size:", display)
            .with_starting_cursor(ctx_idx)
            .prompt()?;

        let chosen = final_ctx_str
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<u64>()?;
        Some(chosen)
    };

    // --- Overwrite check ---
    let overwrite = if helpers::find_config_path(&name).is_some() {
        Confirm::new(&format!("Config \"{}\" exists. Overwrite?", name))
            .with_default(false)
            .prompt()?
    } else {
        true
    };

    if !overwrite {
        return Ok(());
    }

    let result = generator::generate_config(ConfigGenParams {
        name: name.clone(),
        model_path: model_path.clone(),
        ctx: final_ctx,
        mmproj_path,
        overwrite: true,
        n_slots: Some(n_slots),
        enable_fit,
        fit_target_mib: vec![],
    })?;

    println!("\nConfig written to {}", result.path);
    println!("  Model:   {}", result.model_path);
    println!("  Context: {} tokens", result.ctx);
    println!("  GPU:     {}", if has_nvidia || is_apple_silicon { "enabled (99 layers)" } else { "disabled (CPU only)" });
    if enable_fit {
        println!("  Fit:     enabled (min 2048 tokens)");
    }

    Ok(())
}
