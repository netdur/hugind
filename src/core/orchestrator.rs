use crate::core::config::agent::SessionMode;
use crate::core::config::backend::{ResolvedBackend, prepare_backend};
use crate::core::js::runtime::JsRuntime;
use crate::core::wasm::runtime::WasmRuntime;
use crate::shared::logging::{RunLogger, create_agent_logger, create_agent_logger_at};
use semver::{Version, VersionReq};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::path::Path;
use std::path::PathBuf;

pub async fn execute(
    path: String,
    args_vec: Vec<String>,
    cwd_override: Option<String>,
    log_file: Option<String>,
) -> anyhow::Result<()> {
    execute_with_result(path, args_vec, cwd_override, log_file)
        .await
        .map(|_| ())
}

pub async fn execute_with_result(
    path: String,
    args_vec: Vec<String>,
    cwd_override: Option<String>,
    log_file: Option<String>,
) -> anyhow::Result<serde_json::Value> {
    let target_path = PathBuf::from(&path)
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("Error resolving path {}: {}", path, e))?;

    if target_path.is_file() && target_path.extension().and_then(|s| s.to_str()) == Some("yaml") {
        if let Ok(workflow) =
            crate::core::config::workflow::WorkflowConfig::load_from_file(&target_path)
        {
            return run_workflow(
                workflow,
                target_path.parent().unwrap().to_path_buf(),
                args_vec,
                log_file,
            )
            .await;
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
        (
            root,
            target_path,
            crate::core::config::agent::AgentConfig::default(),
        )
    };
    let runtime_cwd = resolve_runtime_cwd(&agent_root, &config, cwd_override)?;
    if runtime_cwd != agent_root {
        if let Some(perms) = config.permissions.as_mut() {
            if let Some(shell) = perms.shell.as_mut() {
                if shell.working_dir.is_none() {
                    shell.working_dir = Some(runtime_cwd.to_string_lossy().into_owned());
                }
            }
        }
    }

    let logger = init_agent_logger(&config, &agent_root, log_file.as_deref());
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
    if let Err(_) = reqwest::get(&backend.health_url)
        .await
        .and_then(|r| r.error_for_status())
    {
        eprintln!(
            "Error: Server is not up or healthy at {}. Aborting agent run.",
            backend.health_url
        );
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
        "args": args_vec,
        "meta": {
            "session": config.runtime_session.clone(),
            "env": resolve_runtime_env(&config)?,
        }
    });

    let run_result = if entry_path.extension().and_then(|s| s.to_str()) == Some("wasm") {
        let wasm = WasmRuntime::new(
            agent_root.clone(),
            runtime_cwd.clone(),
            &config,
            logger.clone(),
        )
        .map_err(|e| anyhow::anyhow!("Runtime error: {}", e))?;
        wasm.run_module(&entry_path, initial_data)
            .await
            .map_err(|e| anyhow::anyhow!("Execution error: {}", e))
    } else {
        let js = JsRuntime::new(
            agent_root.clone(),
            runtime_cwd.clone(),
            &config,
            logger.clone(),
        )
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

    cleanup_fresh_session(&backend, &config).await;

    let output = run_result?;
    Ok(output)
}

async fn run_workflow(
    workflow: crate::core::config::workflow::WorkflowConfig,
    root: PathBuf,
    initial_args: Vec<String>,
    log_file: Option<String>,
) -> anyhow::Result<serde_json::Value> {
    println!("Starting workflow: {}", workflow.name);

    let mut last_output = serde_json::json!({
        "args": initial_args
    });

    for step in workflow.steps {
        println!("==> Step: {}", step.name);
        let agent_dir = root.join(&step.agent);
        let mut config = crate::core::config::agent::AgentConfig::load_from_dir(&agent_dir)?;
        let logger = init_agent_logger(&config, &agent_dir, log_file.as_deref());
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
            let wasm = WasmRuntime::new(
                agent_dir.clone(),
                agent_dir.clone(),
                &config,
                logger.clone(),
            )
            .map_err(|e| anyhow::anyhow!("Runtime error: {}", e))?;
            let res = wasm
                .run_module(&entry, last_output)
                .await
                .map_err(|e| anyhow::anyhow!("Execution error in step {}: {}", step.name, e));
            if let Some(l) = &logger {
                match &res {
                    Ok(_) => l.log_line("agent.run.complete status=ok"),
                    Err(err) => {
                        l.log_line(format!("agent.run.complete status=error error={}", err))
                    }
                }
            }
            cleanup_fresh_session(&backend, &config).await;
            last_output = res?;
        } else {
            let js = JsRuntime::new(
                agent_dir.clone(),
                agent_dir.clone(),
                &config,
                logger.clone(),
            )
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
                    Err(err) => {
                        l.log_line(format!("agent.run.complete status=error error={}", err))
                    }
                }
            }
            cleanup_fresh_session(&backend, &config).await;
            last_output = res?;
        }
    }

    println!("Workflow completed.");
    Ok(last_output)
}

async fn cleanup_fresh_session(
    backend: &ResolvedBackend,
    config: &crate::core::config::agent::AgentConfig,
) {
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
    match client.delete(&url).send().await {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() || status == reqwest::StatusCode::NOT_FOUND {
                return;
            }
            eprintln!("Warning: failed to delete session {}: HTTP {}", id, status);
        }
        Err(e) => {
            eprintln!("Warning: failed to delete session {}: {}", id, e);
        }
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
    log_file: Option<&str>,
) -> Option<RunLogger> {
    let name = log_agent_name(config, agent_root);
    let logger_result = match log_file {
        Some(path) => create_agent_logger_at(path),
        None => create_agent_logger(&name),
    };
    match logger_result {
        Ok(logger) => Some(logger),
        Err(err) => {
            eprintln!("Warning: failed to initialize agent log: {}", err);
            None
        }
    }
}

fn log_agent_name(config: &crate::core::config::agent::AgentConfig, agent_root: &Path) -> String {
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

fn resolve_runtime_cwd(
    agent_root: &Path,
    config: &crate::core::config::agent::AgentConfig,
    cwd_override: Option<String>,
) -> anyhow::Result<PathBuf> {
    let Some(raw) = cwd_override else {
        return Ok(agent_root.to_path_buf());
    };

    let cwd = PathBuf::from(raw)
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("Invalid --cwd path: {}", e))?;

    if cwd.starts_with(agent_root) {
        return Ok(cwd);
    }

    let allow_outside = config
        .permissions
        .as_ref()
        .and_then(|p| p.filesystem.as_ref())
        .map(|f| f.allow_outside_agent_root)
        .unwrap_or(false);

    if allow_outside {
        Ok(cwd)
    } else {
        Err(anyhow::anyhow!(
            "--cwd '{}' is outside agent root '{}' but permissions.filesystem.allow_outside_agent_root is false",
            cwd.display(),
            agent_root.display()
        ))
    }
}

fn resolve_runtime_env(
    config: &crate::core::config::agent::AgentConfig,
) -> anyhow::Result<JsonMap<String, JsonValue>> {
    let mut env_values = JsonMap::new();
    let Some(entries) = &config.env else {
        return Ok(env_values);
    };

    for entry in entries {
        let (name, required) = parse_env_entry(entry)?;
        match std::env::var(&name) {
            Ok(value) => {
                env_values.insert(name, JsonValue::String(value));
            }
            Err(_) if required => {
                return Err(anyhow::anyhow!(
                    "Missing required environment variable '{}' declared in agent.yaml env section",
                    name
                ));
            }
            Err(_) => {}
        }
    }

    Ok(env_values)
}

fn parse_env_entry(entry: &serde_yaml::Value) -> anyhow::Result<(String, bool)> {
    match entry {
        serde_yaml::Value::String(name) => {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                return Err(anyhow::anyhow!("Invalid env entry: empty name"));
            }
            Ok((trimmed.to_string(), false))
        }
        serde_yaml::Value::Mapping(map) => {
            let name_key = serde_yaml::Value::String("name".to_string());
            let required_key = serde_yaml::Value::String("required".to_string());

            let name = map
                .get(&name_key)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("Invalid env entry: missing non-empty name"))?;

            let required = map
                .get(&required_key)
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            Ok((name.to_string(), required))
        }
        _ => Err(anyhow::anyhow!(
            "Invalid env entry: expected string or mapping with name/required"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_env_entry, resolve_runtime_env};
    use crate::core::config::agent::AgentConfig;
    use serde_yaml::Value as YamlValue;

    #[test]
    fn parse_env_entry_accepts_string_name() {
        let (name, required) = parse_env_entry(&YamlValue::String("  API_KEY  ".to_string()))
            .expect("valid env string");
        assert_eq!(name, "API_KEY");
        assert!(!required);
    }

    #[test]
    fn parse_env_entry_accepts_mapping_name_and_required() {
        let value = serde_yaml::from_str::<YamlValue>(
            r#"
name: TOKEN
required: true
"#,
        )
        .expect("valid yaml");

        let (name, required) = parse_env_entry(&value).expect("valid env mapping");
        assert_eq!(name, "TOKEN");
        assert!(required);
    }

    #[test]
    fn parse_env_entry_rejects_invalid_shapes() {
        let missing_name = serde_yaml::from_str::<YamlValue>("required: true").expect("yaml");
        let err = parse_env_entry(&missing_name).expect_err("missing name should fail");
        assert!(err.to_string().contains("missing non-empty name"));

        let err = parse_env_entry(&YamlValue::Number(1u64.into())).expect_err("invalid type");
        assert!(err.to_string().contains("expected string or mapping"));
    }

    #[test]
    fn resolve_runtime_env_returns_empty_when_no_entries() {
        let config = AgentConfig::default();
        let env = resolve_runtime_env(&config).expect("should succeed");
        assert!(env.is_empty());
    }

    #[test]
    fn resolve_runtime_env_errors_for_missing_required_var() {
        let mut config = AgentConfig::default();
        let var_name = format!("HUGIND_TEST_MISSING_{}", std::process::id());
        config.env = Some(vec![
            serde_yaml::from_str::<YamlValue>(&format!("name: {var_name}\nrequired: true\n"))
                .expect("yaml"),
        ]);

        let err = resolve_runtime_env(&config).expect_err("missing required var should fail");
        assert!(err.to_string().contains(&var_name));
    }

    #[test]
    fn resolve_runtime_env_omits_missing_optional_var() {
        let mut config = AgentConfig::default();
        let var_name = format!("HUGIND_TEST_OPTIONAL_{}", std::process::id());
        config.env = Some(vec![YamlValue::String(var_name)]);

        let env = resolve_runtime_env(&config).expect("optional missing should pass");
        assert!(env.is_empty());
    }

    #[test]
    fn resolve_runtime_env_includes_present_variable() {
        let (key, value) = std::env::vars()
            .next()
            .expect("environment should not be empty");
        let mut config = AgentConfig::default();
        config.env = Some(vec![YamlValue::String(key.clone())]);

        let env = resolve_runtime_env(&config).expect("should include variable");
        assert_eq!(env.get(&key).and_then(|v| v.as_str()), Some(value.as_str()));
    }
}
