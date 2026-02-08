use std::path::PathBuf;
use crate::core::js::runtime::JsRuntime;
use crate::core::wasm::runtime::WasmRuntime;
use crate::core::config::backend::{prepare_backend, ResolvedBackend};
use crate::core::config::agent::SessionMode;

pub async fn execute(path: String, args_vec: Vec<String>) -> anyhow::Result<()> {
    let target_path = PathBuf::from(&path).canonicalize().map_err(|e| {
        anyhow::anyhow!("Error resolving path {}: {}", path, e)
    })?;

    if target_path.is_file() && target_path.extension().and_then(|s| s.to_str()) == Some("yaml") {
        
        if let Ok(workflow) = crate::core::config::workflow::WorkflowConfig::load_from_file(&target_path) {
            return run_workflow(workflow, target_path.parent().unwrap().to_path_buf(), args_vec).await;
        }
    }

    let (agent_root, entry_path, mut config) = if target_path.is_dir() {
        
        let config = crate::core::config::agent::AgentConfig::load_from_dir(&target_path)?;
        let entry = target_path.join(&config.entry_point);
        (target_path, entry, config)
    } else {
        
        let root = target_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid entry path"))?
            .to_path_buf();
        (root, target_path, crate::core::config::agent::AgentConfig::default())
    };

    
    
    
    let backend = prepare_backend(&mut config)?;
    println!("Checking server health at {}...", backend.health_url);
    if let Err(_) = reqwest::get(&backend.health_url).await.and_then(|r| r.error_for_status()) {
        eprintln!("Error: Server is not up or healthy at {}. Aborting agent run.", backend.health_url);
        return Err(anyhow::anyhow!("Server health check failed"));
    }
    println!("Server is up. Starting agent...");

    let initial_data = serde_json::json!({
        "args": args_vec
    });

    if entry_path.extension().and_then(|s| s.to_str()) == Some("wasm") {
        let wasm = WasmRuntime::new(agent_root, &config).map_err(|e| anyhow::anyhow!("Runtime error: {}", e))?;
        wasm.run_module(&entry_path, initial_data)
            .await
            .map_err(|e| anyhow::anyhow!("Execution error: {}", e))?;
    } else {
        let js = JsRuntime::new(agent_root, &config).await.map_err(|e| anyhow::anyhow!("Runtime error: {}", e))?;
        js.run_module(&entry_path, initial_data)
            .await
            .map_err(|e| anyhow::anyhow!("Execution error: {}", e))?;
        js.wait_idle().await;
    }

    cleanup_fresh_session(&backend, &config).await;

    Ok(())
}

async fn run_workflow(workflow: crate::core::config::workflow::WorkflowConfig, root: PathBuf, initial_args: Vec<String>) -> anyhow::Result<()> {
    println!("Starting workflow: {}", workflow.name);
    
    let mut last_output = serde_json::json!({
        "args": initial_args
    });

    for step in workflow.steps {
        println!("==> Step: {}", step.name);
        let agent_dir = root.join(&step.agent);
        let mut config = crate::core::config::agent::AgentConfig::load_from_dir(&agent_dir)?;
        let backend = prepare_backend(&mut config)?;
        let entry = agent_dir.join(&config.entry_point);
        
        if entry.extension().and_then(|s| s.to_str()) == Some("wasm") {
            let wasm = WasmRuntime::new(agent_dir, &config).map_err(|e| anyhow::anyhow!("Runtime error: {}", e))?;
            last_output = wasm
                .run_module(&entry, last_output)
                .await
                .map_err(|e| anyhow::anyhow!("Execution error in step {}: {}", step.name, e))?;
        } else {
            let js = JsRuntime::new(agent_dir, &config).await.map_err(|e| anyhow::anyhow!("Runtime error: {}", e))?;
            last_output = js
                .run_module(&entry, last_output)
                .await
                .map_err(|e| anyhow::anyhow!("Execution error in step {}: {}", step.name, e))?;
            js.wait_idle().await;
        }

        cleanup_fresh_session(&backend, &config).await;
    }

    println!("Workflow completed.");
    Ok(())
}

async fn cleanup_fresh_session(backend: &ResolvedBackend, config: &crate::core::config::agent::AgentConfig) {
    let session = match &config.runtime_session {
        Some(s) if s.mode == SessionMode::Fresh => s,
        _ => return,
    };
    let id = match &session.id {
        Some(id) => id,
        None => return,
    };
    let url = format!("{}/state/{}", backend.base_url.trim_end_matches('/'), id);
    let client = reqwest::Client::new();
    if let Err(e) = client.delete(&url).send().await.and_then(|r| r.error_for_status()) {
        eprintln!("Warning: failed to delete session {}: {}", id, e);
    }
}
