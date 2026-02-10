use std::path::PathBuf;
use crate::core::js::runtime::JsRuntime;
use crate::core::wasm::runtime::WasmRuntime;
use crate::core::config::backend::{prepare_backend, ResolvedBackend};
use crate::core::config::agent::SessionMode;
use crate::shared::logging::{create_agent_logger, RunLogger};
use semver::{Version, VersionReq};
use std::path::Path;

pub async fn execute(path: String, args_vec: Vec<String>) -> anyhow::Result<()> {
    execute_with_result(path, args_vec).await.map(|_| ())
}

pub async fn execute_with_result(path: String, args_vec: Vec<String>) -> anyhow::Result<serde_json::Value> {
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

    let logger = init_agent_logger(&config, &agent_root);
    if let Some(l) = &logger {
        let args_json = serde_json::to_string(&args_vec).unwrap_or_default();
        l.log_line(format!(
            "agent.run.start name={} entry={} args_len={} args={}",
            log_agent_name(&config, &agent_root),
            entry_path.display(),
            args_json.len(),
            args_json
        ));
    }

    enforce_hugind_version(&config)?;
    let backend = prepare_backend(&mut config)?;
    println!("Checking server health at {}...", backend.health_url);
    if let Err(_) = reqwest::get(&backend.health_url).await.and_then(|r| r.error_for_status()) {
        eprintln!("Error: Server is not up or healthy at {}. Aborting agent run.", backend.health_url);
        if let Some(l) = &logger {
            l.log_line(format!(
                "agent.run.error server_health_check_failed url={}",
                backend.health_url
            ));
        }
        return Err(anyhow::anyhow!("Server health check failed"));
    }
    println!("Server is up. Starting agent...");

    let initial_data = serde_json::json!({
        "args": args_vec
    });

    let run_result = if entry_path.extension().and_then(|s| s.to_str()) == Some("wasm") {
        let wasm = WasmRuntime::new(agent_root, &config, logger.clone())
            .map_err(|e| anyhow::anyhow!("Runtime error: {}", e))?;
        wasm.run_module(&entry_path, initial_data)
            .await
            .map_err(|e| anyhow::anyhow!("Execution error: {}", e))
    } else {
        let js = JsRuntime::new(agent_root, &config, logger.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Runtime error: {}", e))?;
        let res = js
            .run_module(&entry_path, initial_data)
            .await
            .map_err(|e| anyhow::anyhow!("Execution error: {}", e));
        js.wait_idle().await;
        res
    };

    if let Some(l) = &logger {
        match &run_result {
            Ok(_) => l.log_line("agent.run.complete status=ok"),
            Err(err) => l.log_line(format!("agent.run.complete status=error error={}", err)),
        }
    }

    let output = run_result?;

    cleanup_fresh_session(&backend, &config).await;

    Ok(output)
}

async fn run_workflow(workflow: crate::core::config::workflow::WorkflowConfig, root: PathBuf, initial_args: Vec<String>) -> anyhow::Result<serde_json::Value> {
    println!("Starting workflow: {}", workflow.name);
    
    let mut last_output = serde_json::json!({
        "args": initial_args
    });

    for step in workflow.steps {
        println!("==> Step: {}", step.name);
        let agent_dir = root.join(&step.agent);
        let mut config = crate::core::config::agent::AgentConfig::load_from_dir(&agent_dir)?;
        let logger = init_agent_logger(&config, &agent_dir);
        if let Some(l) = &logger {
            let args_json = serde_json::to_string(&last_output).unwrap_or_default();
            l.log_line(format!(
                "agent.run.start name={} entry={} args_len={}",
                log_agent_name(&config, &agent_dir),
                config.entry_point,
                args_json.len()
            ));
        }
        enforce_hugind_version(&config)?;
        let backend = prepare_backend(&mut config)?;
        let entry = agent_dir.join(&config.entry_point);
        
        if entry.extension().and_then(|s| s.to_str()) == Some("wasm") {
            let wasm = WasmRuntime::new(agent_dir, &config, logger.clone())
                .map_err(|e| anyhow::anyhow!("Runtime error: {}", e))?;
            let res = wasm
                .run_module(&entry, last_output)
                .await
                .map_err(|e| anyhow::anyhow!("Execution error in step {}: {}", step.name, e));
            if let Some(l) = &logger {
                match &res {
                    Ok(_) => l.log_line("agent.run.complete status=ok"),
                    Err(err) => l.log_line(format!("agent.run.complete status=error error={}", err)),
                }
            }
            last_output = res?;
        } else {
            let js = JsRuntime::new(agent_dir, &config, logger.clone())
                .await
                .map_err(|e| anyhow::anyhow!("Runtime error: {}", e))?;
            let res = js
                .run_module(&entry, last_output)
                .await
                .map_err(|e| anyhow::anyhow!("Execution error in step {}: {}", step.name, e));
            js.wait_idle().await;
            if let Some(l) = &logger {
                match &res {
                    Ok(_) => l.log_line("agent.run.complete status=ok"),
                    Err(err) => l.log_line(format!("agent.run.complete status=error error={}", err)),
                }
            }
            last_output = res?;
        }

        cleanup_fresh_session(&backend, &config).await;
    }

    println!("Workflow completed.");
    Ok(last_output)
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

fn enforce_hugind_version(config: &crate::core::config::agent::AgentConfig) -> anyhow::Result<()> {
    let Some(req_str) = &config.hugind_version else {
        return Ok(());
    };
    if req_str.trim().is_empty() {
        return Ok(());
    }

    let req = VersionReq::parse(req_str)
        .map_err(|e| anyhow::anyhow!("Invalid hugind_version constraint '{}': {}", req_str, e))?;
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|e| anyhow::anyhow!("Invalid current Hugind version: {}", e))?;
    if !req.matches(&current) {
        return Err(anyhow::anyhow!(
            "Agent requires hugind_version '{}' but current is {}",
            req_str,
            current
        ));
    }
    Ok(())
}

fn init_agent_logger(
    config: &crate::core::config::agent::AgentConfig,
    agent_root: &Path,
) -> Option<RunLogger> {
    let name = log_agent_name(config, agent_root);
    match create_agent_logger(&name) {
        Ok(logger) => Some(logger),
        Err(err) => {
            eprintln!("Warning: failed to initialize agent log: {}", err);
            None
        }
    }
}

fn log_agent_name(
    config: &crate::core::config::agent::AgentConfig,
    agent_root: &Path,
) -> String {
    let name = config.name.trim();
    if !name.is_empty() && name != "default" {
        return name.to_string();
    }
    agent_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("agent")
        .to_string()
}
