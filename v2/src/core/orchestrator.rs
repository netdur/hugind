use std::path::PathBuf;
use crate::core::js::runtime::JsRuntime;
use crate::core::wasm::runtime::WasmRuntime;
use crate::core::config::backend::resolve_backend;

pub async fn execute(path: String, args_vec: Vec<String>) -> anyhow::Result<()> {
    let target_path = PathBuf::from(&path).canonicalize().map_err(|e| {
        anyhow::anyhow!("Error resolving path {}: {}", path, e)
    })?;

    if target_path.is_file() && target_path.extension().and_then(|s| s.to_str()) == Some("yaml") {
        // Check if it's a workflow
        if let Ok(workflow) = crate::core::config::workflow::WorkflowConfig::load_from_file(&target_path) {
            return run_workflow(workflow, target_path.parent().unwrap().to_path_buf(), args_vec).await;
        }
    }

    let (agent_root, entry_path, config) = if target_path.is_dir() {
        // It's an agent directory, look for agent.yaml
        let config = crate::core::config::agent::AgentConfig::load_from_dir(&target_path)?;
        let entry = target_path.join(&config.entry_point);
        (target_path, entry, config)
    } else {
        // It's a direct file path (legacy/simple mode)
        let root = target_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid entry path"))?
            .to_path_buf();
        (root, target_path, crate::core::config::agent::AgentConfig::default())
    };

    // Server Health Check - logic duplicated from CLI for now, could be moved to shared/utils
    // ideally the orchestrator shouldn't know about HTTP checks unless it's an explicit step, 
    // but preserving behavior for now.
    let backend = resolve_backend(&config)?;
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
        let config = crate::core::config::agent::AgentConfig::load_from_dir(&agent_dir)?;
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
    }

    println!("Workflow completed.");
    Ok(())
}
