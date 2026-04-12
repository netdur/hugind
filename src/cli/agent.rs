use anyhow::{Context, Result};
use inquire::Confirm;
use reqwest::Url;
use std::fs;
use std::io::{self};
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use zip::read::ZipArchive;

use crate::core::config::agent::{
    AgentConfig, FileSystemPermission, NetPermissions, Permissions, ShellPermission,
};
use crate::core::orchestrator::memory::SharedMemory;
use crate::core::orchestrator::messaging::MessageBus;
use crate::shared::paths;

pub async fn run(
    path: String,
    cwd: Option<String>,
    log_file: Option<String>,
    args_vec: Vec<String>,
) -> Result<()> {
    let resolved = resolve_agent_path(&path)?;
    let resolved_args = if cwd.is_some() {
        args_vec
    } else {
        resolve_args_paths(args_vec)?
    };
    crate::core::orchestrator::execute(resolved, resolved_args, cwd, log_file).await
}

pub async fn install(path: String) -> Result<()> {
    let (source_root, config, _temp_guard) = if is_url(&path) {
        if is_zip_path(&path) {
            download_zip_agent(&path).await?
        } else {
            download_agent(&path).await?
        }
    } else if is_zip_path(&path) {
        let root = extract_local_zip_agent(&path)?;
        let config = AgentConfig::load_from_dir(&root)?;
        (root, config, None)
    } else {
        let root = resolve_local_agent_root(&path)?;
        let config = AgentConfig::load_from_dir(&root)?;
        (root, config, None)
    };

    print_permissions(&config.permissions)?;
    let confirm = Confirm::new("Grant these permissions and install this agent?")
        .with_default(false)
        .prompt()?;
    if !confirm {
        println!("Installation cancelled.");
        return Ok(());
    }

    let dest_dir = paths::agents_dir().join(sanitize_agent_name(&config.name));
    if dest_dir.exists() {
        let overwrite = Confirm::new(&format!(
            "Agent already exists at {}. Overwrite?",
            dest_dir.display()
        ))
        .with_default(false)
        .prompt()?;
        if !overwrite {
            println!("Installation cancelled.");
            return Ok(());
        }
        fs::remove_dir_all(&dest_dir)
            .with_context(|| format!("Failed to remove {}", dest_dir.display()))?;
    }

    fs::create_dir_all(&dest_dir)
        .with_context(|| format!("Failed to create {}", dest_dir.display()))?;
    copy_dir_recursive(&source_root, &dest_dir)?;

    println!(
        "✅ Installed agent '{}' to {}",
        config.name,
        dest_dir.display()
    );
    Ok(())
}

pub fn remove(name: String) -> Result<()> {
    let dir = paths::agents_dir();
    let sanitized = sanitize_agent_name(&name);
    let target = dir.join(&sanitized);

    if !target.exists() {
        return Err(anyhow::anyhow!(
            "Agent '{}' not found at {}",
            name,
            target.display()
        ));
    }

    fs::remove_dir_all(&target)
        .with_context(|| format!("Failed to remove {}", target.display()))?;
    println!("Removed agent '{}' from {}", sanitized, dir.display());
    Ok(())
}

pub fn list() -> Result<()> {
    let dir = paths::agents_dir();
    if !dir.exists() {
        println!("No installed agents.");
        return Ok(());
    }

    let mut names = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
    }

    if names.is_empty() {
        println!("No installed agents.");
        return Ok(());
    }

    names.sort();
    println!("{:<24} {}", "NAME", "PATH");
    for name in names {
        let path = dir.join(&name);
        println!("{:<24} {}", name, path.display());
    }
    Ok(())
}

pub async fn team(
    goal: String,
    agents_csv: String,
    backend: Option<String>,
    _concurrency: Option<usize>,
) -> Result<()> {
    use crate::core::config::backend::prepare_backend;
    use crate::core::orchestrator::coordinator;
    use crate::core::orchestrator::events::{EventBus, EventKind};
    use crate::core::orchestrator::memory::SharedMemory;
    use crate::core::orchestrator::messaging::MessageBus;
    use crate::core::orchestrator::task::{Task, TaskQueue, TaskStatus};

    // Capture the user's current working directory — this is where agents should operate
    let user_cwd = std::env::current_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?;

    // Create a shared fresh session for the team — all agents reuse the same KV cache
    let team_session_id = format!("team-{}", uuid::Uuid::new_v4());

    let agent_paths: Vec<String> = agents_csv
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if agent_paths.is_empty() {
        anyhow::bail!("No agents specified. Use --agents agent1,agent2,...");
    }

    // Resolve agent directories and build roster
    let mut roster: Vec<(String, String)> = Vec::new();
    let mut agent_dirs: Vec<(String, PathBuf)> = Vec::new();

    for agent_path in &agent_paths {
        let resolved = resolve_agent_path(agent_path)?;
        let dir = PathBuf::from(&resolved)
            .canonicalize()
            .with_context(|| format!("Agent path not found: {}", agent_path))?;
        let config = AgentConfig::load_from_dir(&dir).unwrap_or_default();
        let name = if config.name.trim().is_empty() || config.name == "default" {
            dir.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("agent")
                .to_string()
        } else {
            config.name.clone()
        };
        // Use agent description if available, otherwise empty
        let description = String::new();
        roster.push((name.clone(), description));
        agent_dirs.push((name, dir));
    }

    println!("Team: {}", roster.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", "));
    println!("Goal: {}\n", goal);

    // Step 1: Send goal to coordinator LLM for decomposition
    println!("Decomposing goal into tasks...");

    let coordinator_prompt = coordinator::build_coordinator_prompt(&roster);
    let user_message = goal.clone();

    // Build LLM request to the coordinator backend
    let mut coord_config = AgentConfig::default();
    if let Some(ref backend_name) = backend {
        coord_config.backend = Some(serde_yaml::Value::Mapping({
            let mut m = serde_yaml::Mapping::new();
            m.insert(
                serde_yaml::Value::String("config".to_string()),
                serde_yaml::Value::String(backend_name.clone()),
            );
            m
        }));
    }
    let coord_backend = prepare_backend(&mut coord_config)?;

    let llm_url = format!(
        "{}/chat/completions",
        coord_backend.base_url.trim_end_matches('/')
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let body = serde_json::json!({
        "model": coord_backend.model.as_deref().unwrap_or("default"),
        "stream": false,
        "messages": [
            { "role": "system", "content": coordinator_prompt },
            { "role": "user", "content": user_message }
        ],
        "response_format": { "type": "json_object" }
    });

    let resp = client.post(&llm_url).json(&body).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Coordinator LLM request failed: {} {}", status, text);
    }

    let resp_json: serde_json::Value = resp.json().await?;
    let coordinator_output = resp_json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| anyhow::anyhow!("Coordinator returned no content"))?;

    let tasks = coordinator::parse_coordinator_tasks(coordinator_output)?;
    println!("Coordinator created {} tasks:", tasks.len());
    for task in &tasks {
        let deps = if task.depends_on.is_empty() {
            String::new()
        } else {
            format!(" (after: {})", task.depends_on.join(", "))
        };
        println!(
            "  - {} → {}{}",
            task.title,
            task.assignee.as_deref().unwrap_or("?"),
            deps
        );
    }
    println!();

    // Step 2: Build task queue and execute
    let events = EventBus::new();
    let memory = SharedMemory::new();
    let messages = MessageBus::new();

    events.emit(EventKind::WorkflowStart {
        name: format!("team: {}", goal),
    });

    let mut queue = TaskQueue::new();
    queue.load_tasks(tasks)?;

    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(
        _concurrency.unwrap_or(4),
    ));

    // Map agent names to directories
    let agent_dir_map: std::collections::HashMap<String, PathBuf> = agent_dirs.into_iter().collect();

    loop {
        if queue.is_done() {
            break;
        }

        let ready: Vec<Task> = queue.next_ready().iter().map(|t| (*t).clone()).collect();
        if ready.is_empty() {
            if queue.has_in_progress() {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            }
            break;
        }

        let mut handles = Vec::new();

        for task in ready {
            let task_id = task.id.clone();
            queue.start(&task_id);

            println!(
                "  [{}] Starting: {} ({})",
                task.id,
                task.title,
                task.assignee.as_deref().unwrap_or("?")
            );

            let memory = memory.clone();
            let messages = messages.clone();
            let sem = semaphore.clone();
            let dir_map = agent_dir_map.clone();
            let cwd = user_cwd.clone();
            let session_id = team_session_id.clone();

            let handle = std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");

                rt.block_on(async move {
                    let _permit = sem.acquire().await;

                    let agent_name = task.assignee.as_deref().unwrap_or("default");
                    let agent_dir = match dir_map.get(agent_name) {
                        Some(d) => d.clone(),
                        None => {
                            return (
                                task.id.clone(),
                                task.title.clone(),
                                agent_name.to_string(),
                                Err(anyhow::anyhow!("Agent '{}' not found in team", agent_name)),
                            );
                        }
                    };

                    let mut config =
                        AgentConfig::load_from_dir(&agent_dir).unwrap_or_default();
                    let entry = agent_dir.join(&config.entry_point);

                    // Set shared team session — fresh, deleted after team completes
                    config.runtime_session = Some(crate::core::config::agent::RuntimeSession {
                        mode: crate::core::config::agent::SessionMode::Fresh,
                        id: Some(session_id.clone()),
                    });

                    // Override shell working_dir to user's cwd
                    if let Some(perms) = config.permissions.as_mut() {
                        if let Some(shell) = perms.shell.as_mut() {
                            if shell.working_dir.is_none() {
                                shell.working_dir = Some(cwd.to_string_lossy().into_owned());
                            }
                        }
                    }

                    let task_args = vec![
                        "--task-id".to_string(), task.id.clone(),
                        "--task-title".to_string(), task.title.clone(),
                        "--task-description".to_string(), task.description.clone(),
                    ];

                    let team_ctx = crate::core::orchestrator::context::TeamContext::new(
                        agent_name, memory.clone(), messages.clone(),
                    );

                    // Use user's cwd as the runtime working directory, not the agent's own dir
                    let result = run_agent_with_team(
                        &agent_dir, &entry, &cwd, &mut config,
                        task_args, None, &memory, &messages, &team_ctx,
                    ).await;

                    (task.id.clone(), task.title.clone(), agent_name.to_string(), result)
                })
            });

            handles.push((task_id, handle));
        }

        for (task_id, handle) in handles {
            match handle.join() {
                Ok((id, title, agent, Ok(result))) => {
                    memory.set(&agent, &title, result.clone());
                    queue.complete(&id, result);
                    println!("  [{}] Completed: {}", id, title);
                }
                Ok((id, title, _, Err(e))) => {
                    let error = format!("{}", e);
                    queue.fail(&id, &error);
                    eprintln!("  [{}] Failed: {} -- {}", id, title, error);
                }
                Err(_) => {
                    queue.fail(&task_id, "Task thread panicked");
                }
            }
        }
    }

    // Step 3: Synthesis
    let failed: Vec<&crate::core::orchestrator::task::Task> = queue
        .all_tasks()
        .into_iter()
        .filter(|t| t.status == TaskStatus::Failed)
        .collect();
    let success = failed.is_empty();

    if success {
        println!("\nAll tasks completed. Synthesizing result...");
        let synthesis_prompt = coordinator::build_synthesis_prompt(&memory);
        let synth_body = serde_json::json!({
            "model": coord_backend.model.as_deref().unwrap_or("default"),
            "stream": false,
            "messages": [
                { "role": "system", "content": "You are a project coordinator. Summarize the team's work." },
                { "role": "user", "content": synthesis_prompt }
            ]
        });

        if let Ok(resp) = client.post(&llm_url).json(&synth_body).send().await {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(content) = json
                    .get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("message"))
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str())
                {
                    println!("\n{}", content);
                }
            }
        }
    } else {
        eprintln!("\nTeam completed with failures:");
        for task in &failed {
            eprintln!(
                "  - {}: {}",
                task.title,
                task.error.as_deref().unwrap_or("unknown")
            );
        }
    }

    events.emit(EventKind::WorkflowComplete { success });

    // Clean up the shared team session
    let mut cleanup_config = AgentConfig::default();
    if let Some(ref backend_name) = backend {
        cleanup_config.backend = Some(serde_yaml::Value::Mapping({
            let mut m = serde_yaml::Mapping::new();
            m.insert(
                serde_yaml::Value::String("config".to_string()),
                serde_yaml::Value::String(backend_name.clone()),
            );
            m
        }));
    }
    cleanup_config.runtime_session = Some(crate::core::config::agent::RuntimeSession {
        mode: crate::core::config::agent::SessionMode::Fresh,
        id: Some(team_session_id.clone()),
    });
    if let Ok(backend) = prepare_backend(&mut cleanup_config) {
        let url = format!("{}/state/{}", backend.base_url.trim_end_matches('/'), team_session_id);
        let _ = reqwest::Client::new().delete(&url).send().await;
    }

    Ok(())
}

/// Run an agent with full team context (used by the team command).
async fn run_agent_with_team(
    agent_root: &Path,
    entry_path: &Path,
    runtime_cwd: &Path,
    config: &mut AgentConfig,
    args_vec: Vec<String>,
    logger: Option<crate::shared::logging::RunLogger>,
    memory: &SharedMemory,
    messages: &MessageBus,
    team_ctx: &crate::core::orchestrator::context::TeamContext,
) -> Result<serde_json::Value> {
    use crate::core::config::backend::prepare_backend;

    if !entry_path.exists() {
        anyhow::bail!("Entry point not found: {}", entry_path.display());
    }

    let backend = prepare_backend(config)?;

    // Health check
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    client
        .get(&backend.health_url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| anyhow::anyhow!("Server not reachable at {}: {}", backend.health_url, e))?;

    let msg_context = messages.format_for_prompt(&team_ctx.agent_name);
    let mem_summary = memory.summary();

    let initial_data = serde_json::json!({
        "args": args_vec,
        "meta": {
            "session": config.runtime_session.clone(),
        },
        "team": {
            "memory": memory.to_json(),
            "memory_summary": mem_summary,
            "messages": msg_context,
        }
    });

    let js = crate::core::js::runtime::JsRuntime::new_with_team(
        agent_root.to_path_buf(),
        runtime_cwd.to_path_buf(),
        config,
        logger.clone(),
        Some(team_ctx),
    )
    .await
    .map_err(|e| anyhow::anyhow!("Runtime error: {}", e))?;

    if config.mode == crate::core::config::agent::AgentMode::Agentic {
        use crate::core::orchestrator::agentic::ToolRegistry;
        use crate::core::js::capabilities::agentic as agentic_cap;
        use crate::core::orchestrator::agentic::{parse_tool_calls, strip_tool_calls};
        use crate::core::config::backend::prepare_backend as prep_backend;

        let registry = ToolRegistry::new();
        agentic_cap::install(js.context(), &registry, logger.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to install agentic globals: {}", e))?;

        // Run entry point to register tools and system prompt
        let _ = js.run_module(entry_path, initial_data).await
            .map_err(|e| anyhow::anyhow!("Agent setup error: {}", e))?;
        js.wait_idle().await;

        // Build user prompt from task args
        let mut user_prompt = String::new();
        if !mem_summary.is_empty() {
            user_prompt.push_str(&mem_summary);
            user_prompt.push_str("\n\n");
        }
        if !msg_context.is_empty() {
            user_prompt.push_str(&msg_context);
            user_prompt.push_str("\n\n");
        }
        // Extract task description from args
        let mut i = 0;
        while i < args_vec.len() {
            if (args_vec[i] == "--task-description" || args_vec[i] == "--prompt") && i + 1 < args_vec.len() {
                user_prompt.push_str(&args_vec[i + 1]);
                break;
            }
            i += 1;
        }
        if user_prompt.trim().is_empty() {
            user_prompt = args_vec.join(" ");
        }

        // Build system prompt with tool descriptions
        let mut system = registry.get_system_prompt().unwrap_or_default();
        let tools_section = registry.tools_prompt();
        if !tools_section.is_empty() {
            system.push_str(&tools_section);
        }

        let backend2 = prep_backend(config)?;
        let llm_url = format!("{}/chat/completions", backend2.base_url.trim_end_matches('/'));
        let model = backend2.model.as_deref().unwrap_or("default").to_string();
        let max_turns = config.max_turns.unwrap_or(10) as usize;
        // NOTE: long request timeout so slow LLM generations don't stall,
        // but finite so a hung server eventually errors out.
        let http_client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let trace = std::env::var("HUGIND_TRACE").map(|v| v == "1" || v == "true").unwrap_or(false);

        let mut messages: Vec<serde_json::Value> = Vec::new();
        if !system.is_empty() {
            messages.push(serde_json::json!({"role": "system", "content": system}));
        }
        messages.push(serde_json::json!({"role": "user", "content": user_prompt}));

        if trace {
            eprintln!("[team-trace] loop: url={} model={} max_turns={}", llm_url, model, max_turns);
            eprintln!("[team-trace] === SYSTEM PROMPT (len={}) ===\n{}\n[team-trace] === END SYSTEM ===", system.len(), system);
            eprintln!("[team-trace] === USER PROMPT (len={}) ===\n{}\n[team-trace] === END USER ===", user_prompt.len(), user_prompt);
        }

        let mut final_text = String::new();

        for turn in 0..max_turns {
            if trace {
                eprintln!("[team-trace] turn {}/{}: sending {} messages", turn, max_turns, messages.len());
            }

            let body = serde_json::json!({
                "model": model,
                "messages": messages,
                "stream": false,
            });

            let mut request = http_client.post(&llm_url).json(&body);
            if let Some(session) = config.runtime_session.as_ref().and_then(|s| s.id.as_deref()) {
                request = request.header("X-Session-ID", session);
            }

            let llm_start = std::time::Instant::now();
            let resp = request.send().await
                .map_err(|e| anyhow::anyhow!("LLM request failed: {}", e))?;
            if trace {
                eprintln!("[team-trace] turn {}: response {} in {:.1}s", turn, resp.status(), llm_start.elapsed().as_secs_f64());
            }
            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                anyhow::bail!("LLM error: {} {}", status, text);
            }

            let resp_json: serde_json::Value = resp.json().await?;
            let content = resp_json.get("choices").and_then(|c| c.get(0))
                .and_then(|c| c.get("message")).and_then(|m| m.get("content"))
                .and_then(|c| c.as_str()).unwrap_or("");
            if trace {
                eprintln!("[team-trace] turn {}: content_len={} preview={}", turn, content.len(), &content[..content.len().min(150)]);
            }

            messages.push(serde_json::json!({"role": "assistant", "content": content}));

            let tool_calls = parse_tool_calls(content);
            if trace {
                eprintln!("[team-trace] turn {}: {} tool calls parsed", turn, tool_calls.len());
            }
            if tool_calls.is_empty() {
                final_text = strip_tool_calls(content);
                break;
            }

            // Execute tool calls locally
            let mut results = Vec::new();
            for tc in &tool_calls {
                let args_str = serde_json::to_string(&tc.args).unwrap_or_default();
                if trace {
                    eprintln!("[team-trace] turn {}: exec {} args={}", turn, tc.name, &args_str[..args_str.len().min(80)]);
                }
                let tool_start = std::time::Instant::now();
                let result = crate::core::orchestrator::runner::execute_js_tool_pub(
                    &js, &tc.name, &args_str,
                ).await;
                let result_str = result.unwrap_or_else(|e| format!("Error: {}", e));
                if trace {
                    eprintln!("[team-trace] turn {}: {} done in {:.1}s len={}", turn, tc.name, tool_start.elapsed().as_secs_f64(), result_str.len());
                }
                results.push(format!("[{}] {}", tc.name, result_str));
            }

            messages.push(serde_json::json!({
                "role": "user",
                "content": format!("Tool results:\n\n{}", results.join("\n\n")),
            }));
        }

        Ok(serde_json::Value::String(final_text))
    } else {
        let result = js
            .run_module(entry_path, initial_data)
            .await
            .map_err(|e| anyhow::anyhow!("Execution error: {}", e));
        js.wait_idle().await;
        result
    }
}

fn is_url(input: &str) -> bool {
    Url::parse(input)
        .map(|u| u.scheme() == "http" || u.scheme() == "https")
        .unwrap_or(false)
}

fn resolve_local_agent_root(path: &str) -> Result<PathBuf> {
    let target = PathBuf::from(path)
        .canonicalize()
        .with_context(|| format!("Failed to resolve path {}", path))?;
    if target.is_file() {
        let name = target.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name == "agent.yaml" {
            return Ok(target
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Invalid agent.yaml path"))?
                .to_path_buf());
        }
        return Err(anyhow::anyhow!(
            "Expected a folder containing agent.yaml or a direct agent.yaml path"
        ));
    }
    Ok(target)
}

fn is_zip_path(path: &str) -> bool {
    path.to_lowercase().ends_with(".zip")
}

fn extract_local_zip_agent(path: &str) -> Result<PathBuf> {
    let zip_path = PathBuf::from(path)
        .canonicalize()
        .with_context(|| format!("Failed to resolve zip path {}", path))?;
    let temp = tempfile::tempdir()?;
    let root = temp.path().to_path_buf();
    extract_zip(&zip_path, &root)?;
    let agent_root = find_agent_root(&root)?;
    Ok(agent_root)
}

async fn download_agent(path: &str) -> Result<(PathBuf, AgentConfig, Option<TempDir>)> {
    let base_url = resolve_agent_base_url(path)?;

    let agent_url = base_url.join("agent.yaml")?;
    let temp = tempfile::tempdir()?;
    let root = temp.path().to_path_buf();

    let agent_yaml = fetch_text(&agent_url)
        .await
        .with_context(|| format!("Failed to download {}", agent_url))?;
    fs::write(root.join("agent.yaml"), agent_yaml)?;

    let config = AgentConfig::load_from_dir(&root)?;
    let entry_url = base_url.join(&config.entry_point)?;
    let entry_path = root.join(&config.entry_point);
    if let Some(parent) = entry_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let entry_bytes = fetch_bytes(&entry_url)
        .await
        .with_context(|| format!("Failed to download {}", entry_url))?;
    fs::write(&entry_path, &entry_bytes)?;

    Ok((root, config, Some(temp)))
}

async fn download_zip_agent(path: &str) -> Result<(PathBuf, AgentConfig, Option<TempDir>)> {
    let url = Url::parse(path)?;
    let temp = tempfile::tempdir()?;
    let root = temp.path().to_path_buf();
    let zip_path = root.join("agent.zip");

    let bytes = fetch_bytes(&url)
        .await
        .with_context(|| format!("Failed to download {}", url))?;
    fs::write(&zip_path, &bytes)?;

    extract_zip(&zip_path, &root)?;
    let agent_root = find_agent_root(&root)?;
    let config = AgentConfig::load_from_dir(&agent_root)?;
    Ok((agent_root, config, Some(temp)))
}

fn resolve_agent_base_url(path: &str) -> Result<Url> {
    let url = Url::parse(path)?;

    if let Some(raw_base) = github_raw_base(&url) {
        return Ok(raw_base);
    }

    if path.ends_with("agent.yaml") {
        return Ok(url.join(".")?);
    }

    if path.ends_with('/') {
        return Ok(url);
    }

    Url::parse(&(path.to_string() + "/")).map_err(Into::into)
}

fn github_raw_base(url: &Url) -> Option<Url> {
    if url.host_str() != Some("github.com") {
        return None;
    }

    let segments: Vec<_> = url.path_segments()?.collect();
    if segments.len() < 4 {
        return None;
    }

    let owner = segments[0];
    let repo = segments[1];
    let kind = segments[2];

    if kind != "tree" && kind != "blob" {
        return None;
    }

    let branch = segments[3];
    let path_parts = &segments[4..];
    let mut base = format!(
        "https://raw.githubusercontent.com/{}/{}/{}/",
        owner, repo, branch
    );

    if !path_parts.is_empty() {
        let mut dir_parts = path_parts.to_vec();
        if kind == "blob" && !dir_parts.is_empty() {
            dir_parts.pop();
        }
        if !dir_parts.is_empty() {
            base.push_str(&dir_parts.join("/"));
            base.push('/');
        }
    }

    Url::parse(&base).ok()
}

async fn fetch_text(url: &Url) -> Result<String> {
    let response = reqwest::get(url.clone()).await?.error_for_status()?;
    Ok(response.text().await?)
}

async fn fetch_bytes(url: &Url) -> Result<Vec<u8>> {
    let response = reqwest::get(url.clone()).await?.error_for_status()?;
    Ok(response.bytes().await?.to_vec())
}

fn sanitize_agent_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "agent".to_string();
    }
    trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<()> {
    let file = fs::File::open(zip_path)
        .with_context(|| format!("Failed to open zip {}", zip_path.display()))?;
    let mut archive =
        ZipArchive::new(file).with_context(|| format!("Invalid zip {}", zip_path.display()))?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(rel_path) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest.join(rel_path);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut outfile = fs::File::create(&out_path)?;
        io::copy(&mut entry, &mut outfile)?;
    }
    Ok(())
}

fn find_agent_root(root: &Path) -> Result<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut found: Option<PathBuf> = None;

    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.file_name().and_then(|s| s.to_str()) == Some("agent.yaml") {
                let candidate = path.parent().unwrap_or(root).to_path_buf();
                if let Some(existing) = &found {
                    if existing != &candidate {
                        return Err(anyhow::anyhow!(
                            "Multiple agent.yaml files found in zip; please provide a zip with a single agent"
                        ));
                    }
                } else {
                    found = Some(candidate);
                }
            }
        }
    }

    found.ok_or_else(|| anyhow::anyhow!("agent.yaml not found in zip"))
}

fn print_permissions(perms: &Option<Permissions>) -> Result<()> {
    println!("\nRequested permissions:");
    if perms.is_none() {
        println!("- No special permissions requested");
        return Ok(());
    }

    let perms = perms.as_ref().unwrap();
    if let Some(net) = &perms.network {
        print_net_permissions(net);
    } else {
        println!("- Network access: No");
    }

    if let Some(fs_perm) = &perms.filesystem {
        print_fs_permissions(fs_perm);
    } else {
        println!("- File access: No");
    }

    if let Some(shell) = &perms.shell {
        print_shell_permissions(shell);
    } else {
        println!("- Run system commands: No");
    }

    Ok(())
}

fn print_net_permissions(net: &NetPermissions) {
    if !net.allow {
        println!("- Network access: No");
        return;
    }

    let mut details = Vec::new();
    if !net.allowed_domains.is_empty() {
        details.push(format!("domains: {}", net.allowed_domains.join(", ")));
    }
    if !net.allowed_ips.is_empty() {
        details.push(format!("ips: {}", net.allowed_ips.join(", ")));
    }
    if net.block_private_networks {
        details.push("blocks private networks".to_string());
    }
    if let Some(v) = &net.max_response_bytes {
        details.push(format!("max response: {}", v));
    }
    if let Some(v) = &net.timeout {
        details.push(format!("timeout: {}", v));
    }

    if details.is_empty() {
        println!("- Network access: Yes");
    } else {
        println!("- Network access: Yes ({})", details.join("; "));
    }
}

fn print_fs_permissions(fs_perm: &FileSystemPermission) {
    if !fs_perm.allow {
        println!("- File access: No");
        return;
    }

    let mut actions = Vec::new();
    if fs_perm.read {
        actions.push("read");
    }
    if fs_perm.write {
        actions.push("write");
    }
    if fs_perm.create {
        actions.push("create");
    }
    if fs_perm.delete {
        actions.push("delete");
    }

    let mut details = Vec::new();
    if !actions.is_empty() {
        details.push(format!("actions: {}", actions.join(", ")));
    }
    if !fs_perm.allowed_paths.is_empty() {
        details.push(format!("paths: {}", fs_perm.allowed_paths.join(", ")));
    }
    if !fs_perm.denied_paths.is_empty() {
        details.push(format!("blocked: {}", fs_perm.denied_paths.join(", ")));
    }
    if fs_perm.allow_outside_agent_root {
        details.push("can access outside agent folder".to_string());
    }
    if fs_perm.follow_symlinks {
        details.push("follows symlinks".to_string());
    }

    if details.is_empty() {
        println!("- File access: Yes");
    } else {
        println!("- File access: Yes ({})", details.join("; "));
    }
}

fn print_shell_permissions(shell: &ShellPermission) {
    if !shell.allow {
        println!("- Run system commands: No");
        return;
    }

    let mut details = Vec::new();
    if let Some(list) = &shell.whitelist {
        if !list.is_empty() {
            details.push(format!("allowed: {}", list.join(", ")));
        }
    }
    if let Some(list) = &shell.blacklist {
        if !list.is_empty() {
            details.push(format!("blocked: {}", list.join(", ")));
        }
    }
    if let Some(v) = &shell.timeout {
        details.push(format!("timeout: {}", v));
    }
    if let Some(v) = &shell.max_output {
        details.push(format!("max output: {}", v));
    }
    if shell.env_clear {
        details.push("clears env".to_string());
    }
    if let Some(v) = &shell.working_dir {
        details.push(format!("working dir: {}", v));
    }

    if details.is_empty() {
        println!("- Run system commands: Yes");
    } else {
        println!("- Run system commands: Yes ({})", details.join("; "));
    }
}

fn resolve_agent_path(path: &str) -> Result<String> {
    let input = PathBuf::from(path);
    if input.exists() {
        return Ok(path.to_string());
    }

    let installed = paths::agents_dir().join(path);
    if installed.exists() {
        return Ok(installed.to_string_lossy().to_string());
    }

    Err(anyhow::anyhow!(
        "Error resolving path {}: No such file or directory",
        path
    ))
}

fn resolve_args_paths(args: Vec<String>) -> Result<Vec<String>> {
    let mut resolved = Vec::with_capacity(args.len());
    for arg in args {
        if arg.starts_with('-') {
            resolved.push(arg);
            continue;
        }
        if arg.contains("://") {
            resolved.push(arg);
            continue;
        }
        let path = PathBuf::from(&arg);
        if path.is_absolute() {
            resolved.push(arg);
            continue;
        }
        if path.exists() {
            let abs = path
                .canonicalize()
                .with_context(|| format!("Failed to resolve path {}", arg))?;
            resolved.push(abs.to_string_lossy().to_string());
        } else {
            resolved.push(arg);
        }
    }
    Ok(resolved)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("Failed to create {}", dst.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("Failed to read {}", src.display()))? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if ty.is_file() {
            fs::copy(&src_path, &dst_path).with_context(|| {
                format!(
                    "Failed to copy {} to {}",
                    src_path.display(),
                    dst_path.display()
                )
            })?;
        }
    }
    Ok(())
}
