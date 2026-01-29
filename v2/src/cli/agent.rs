use crate::core::js::runtime::JsRuntime;
use std::path::PathBuf;

pub async fn run(path: String) -> anyhow::Result<()> {
    let target_path = PathBuf::from(path).canonicalize().map_err(|e| {
        anyhow::anyhow!("Error resolving path: {}", e)
    })?;

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


    // Server Health Check
    let health_url = "http://127.0.0.1:8080/health"; // Default / requested URL
    // Ideally we should derive this from config.backend if present, but user specifically asked for this.
    // Let's print what we are doing.
    println!("Checking server health at {}...", health_url);
    if let Err(_) = reqwest::get(health_url).await.and_then(|r| r.error_for_status()) {
        eprintln!("Error: Server is not up or healthy at {}. Aborting agent run.", health_url);
        // We return OK to exit gracefully without stacktrace? Or Error? User said "only run if server is up".
        // Let's return error so it's clear.
        return Err(anyhow::anyhow!("Server health check failed"));
    }
    println!("Server is up. Starting agent...");

    let js = JsRuntime::new(agent_root, &config).await.map_err(|e| anyhow::anyhow!("Runtime error: {}", e))?;
    js.run_module(&entry_path).await.map_err(|e| anyhow::anyhow!("Execution error: {}", e))?;
    js.wait_idle().await;

    Ok(())
}

pub fn install() -> anyhow::Result<()> {
    println!("Agent install not implemented yet");
    Ok(())
}

pub fn remove() -> anyhow::Result<()> {
    println!("Agent remove not implemented yet");
    Ok(())
}
