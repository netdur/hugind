use crate::core::config::agent::{AgentConfig, SessionMode};
use crate::core::config::backend::{ResolvedBackend, prepare_backend};
use crate::core::config::workflow::WorkflowConfig;
use crate::core::js::runtime::JsRuntime;
use crate::core::orchestrator::context::TeamContext;
use crate::core::orchestrator::events::{EventBus, EventKind};
use crate::core::orchestrator::memory::SharedMemory;
use crate::core::orchestrator::messaging::MessageBus;
use crate::core::orchestrator::task::{Task, TaskQueue, TaskStatus};
use crate::core::wasm::runtime::WasmRuntime;
use crate::shared::logging::{RunLogger, create_agent_logger, create_agent_logger_at};
use semver::{Version, VersionReq};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::path::{Path, PathBuf};

// ── Public API ──────────────────────────────────────────────────────────────

pub async fn execute(
    path: String,
    args_vec: Vec<String>,
    cwd_override: Option<String>,
    log_file: Option<String>,
) -> anyhow::Result<()> {
    let result = execute_with_result(path, args_vec, cwd_override, log_file).await?;

    // Print the result if it's a non-null string (agentic mode output)
    match &result {
        serde_json::Value::String(s) if !s.is_empty() => {
            println!("{}", s);
        }
        serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
            if let Ok(pretty) = serde_json::to_string_pretty(&result) {
                println!("{}", pretty);
            }
        }
        _ => {}
    }

    Ok(())
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

    // Check if this is a workflow file
    if target_path.is_file() && target_path.extension().and_then(|s| s.to_str()) == Some("yaml") {
        match WorkflowConfig::load_from_file(&target_path) {
            Ok(workflow) => {
                let root = target_path
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("Workflow file has no parent directory"))?
                    .to_path_buf();

                if workflow.is_v2() {
                    return run_workflow_v2(workflow, root, args_vec, log_file).await;
                } else {
                    return run_workflow_v1(workflow, root, args_vec, log_file).await;
                }
            }
            Err(_) => {}
        }
    }

    let (agent_root, entry_path, mut config) = if target_path.is_dir() {
        let config = AgentConfig::load_from_dir(&target_path)?;
        let entry = target_path.join(&config.entry_point);
        (target_path, entry, config)
    } else {
        let root = target_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid entry path"))?
            .to_path_buf();
        (root, target_path, AgentConfig::default())
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
    let memory = SharedMemory::new();
    let messages = MessageBus::new();

    let output = run_agent(
        &agent_root,
        &entry_path,
        &runtime_cwd,
        &mut config,
        args_vec,
        logger,
        &memory,
        &messages,
    )
    .await?;

    Ok(output)
}

// ── V1 Workflow (sequential steps, backward compatible) ─────────────────────

async fn run_workflow_v1(
    workflow: WorkflowConfig,
    root: PathBuf,
    initial_args: Vec<String>,
    log_file: Option<String>,
) -> anyhow::Result<serde_json::Value> {
    println!("Starting workflow: {}", workflow.name);

    let memory = SharedMemory::new();
    let messages = MessageBus::new();
    let mut last_output = serde_json::json!({ "args": initial_args });

    for step in workflow.steps {
        println!("==> Step: {}", step.name);
        let agent_dir = root.join(&step.agent);
        let mut config = AgentConfig::load_from_dir(&agent_dir)?;
        let entry = agent_dir.join(&config.entry_point);
        let logger = init_agent_logger(&config, &agent_dir, log_file.as_deref());

        let step_args = if let Some(args) = last_output.get("args").and_then(|a| a.as_array()) {
            args.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else {
            vec![serde_json::to_string(&last_output).unwrap_or_default()]
        };

        let result = run_agent(
            &agent_dir,
            &entry,
            &agent_dir,
            &mut config,
            step_args,
            logger,
            &memory,
            &messages,
        )
        .await
        .map_err(|e| anyhow::anyhow!("Error in step '{}': {}", step.name, e))?;

        let agent_name = log_agent_name(&config, &agent_dir);
        memory.set(&agent_name, &step.name, result.clone());

        last_output = result;
    }

    println!("Workflow completed.");
    Ok(last_output)
}

// ── V2 Workflow (task DAG with parallel execution) ──────────────────────────

async fn run_workflow_v2(
    workflow: WorkflowConfig,
    root: PathBuf,
    initial_args: Vec<String>,
    log_file: Option<String>,
) -> anyhow::Result<serde_json::Value> {
    let events = EventBus::new();
    let memory = SharedMemory::new();
    let messages = MessageBus::new();

    events.emit(EventKind::WorkflowStart {
        name: workflow.name.clone(),
    });

    println!(
        "Starting workflow: {} ({} tasks)",
        workflow.name,
        workflow.tasks.len()
    );

    memory.set("workflow", "initial_args", serde_json::json!(initial_args));

    // Build task queue
    let mut queue = TaskQueue::new();
    let tasks: Vec<Task> = workflow
        .tasks
        .iter()
        .enumerate()
        .map(|(i, wt)| {
            let mut task = Task::new(&format!("task-{}", i), &wt.title, &wt.description);
            task.assignee = Some(wt.agent.clone());
            task.depends_on = wt.depends_on.clone();
            task.backend = wt
                .backend
                .as_ref()
                .and_then(|b| workflow.backends.get(b).cloned().or_else(|| Some(b.clone())));
            task
        })
        .collect();

    queue.load_tasks(tasks)?;

    // Concurrency semaphore
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(4));

    // Execution loop
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

            events.emit(EventKind::TaskStart {
                task_id: task.id.clone(),
                title: task.title.clone(),
                assignee: task.assignee.clone().unwrap_or_default(),
            });

            println!(
                "  [{}] Starting: {} ({})",
                task.id,
                task.title,
                task.assignee.as_deref().unwrap_or("unassigned")
            );

            let root = root.clone();
            let log_file = log_file.clone();
            let memory = memory.clone();
            let messages = messages.clone();
            let sem = semaphore.clone();
            let backends = workflow.backends.clone();

            // JsRuntime (rquickjs) is not Send, so we spawn a dedicated
            // tokio runtime per task on a new OS thread. This also matches
            // Hugind's design: each agent runs in its own OS process/thread.
            let handle = std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");

                rt.block_on(async move {
                    let _permit = sem.acquire().await;

                    let agent_name = task.assignee.as_deref().unwrap_or("default");
                    let agent_dir = root.join(agent_name);
                    let mut config =
                        AgentConfig::load_from_dir(&agent_dir).unwrap_or_default();
                    let entry = agent_dir.join(&config.entry_point);
                    let logger = init_agent_logger(&config, &agent_dir, log_file.as_deref());

                    if let Some(backend_name) = &task.backend {
                        let config_name = backends.get(backend_name).unwrap_or(backend_name);
                        config.backend = Some(serde_yaml::Value::Mapping({
                            let mut m = serde_yaml::Mapping::new();
                            m.insert(
                                serde_yaml::Value::String("config".to_string()),
                                serde_yaml::Value::String(config_name.clone()),
                            );
                            m
                        }));
                    }

                    let task_args = vec![
                        "--task-id".to_string(),
                        task.id.clone(),
                        "--task-title".to_string(),
                        task.title.clone(),
                        "--task-description".to_string(),
                        task.description.clone(),
                    ];

                    let result = run_agent(
                        &agent_dir,
                        &entry,
                        &agent_dir,
                        &mut config,
                        task_args,
                        logger,
                        &memory,
                        &messages,
                    )
                    .await;

                    (
                        task.id.clone(),
                        task.title.clone(),
                        agent_name.to_string(),
                        result,
                    )
                })
            });

            handles.push((task_id, handle));
        }

        // Collect results — handles are std::thread::JoinHandle
        for (task_id, handle) in handles {
            match handle.join() {
                Ok((id, title, agent, Ok(result))) => {
                    memory.set(&agent, &title, result.clone());
                    queue.complete(&id, result);
                    events.emit(EventKind::TaskComplete {
                        task_id: id.clone(),
                        title: title.clone(),
                    });
                    println!("  [{}] Completed: {}", id, title);
                }
                Ok((id, title, _, Err(e))) => {
                    let error = format!("{}", e);
                    queue.fail(&id, &error);
                    events.emit(EventKind::TaskFailed {
                        task_id: id.clone(),
                        title: title.clone(),
                        error: error.clone(),
                    });
                    eprintln!("  [{}] Failed: {} -- {}", id, title, error);
                }
                Err(_) => {
                    queue.fail(&task_id, "Task thread panicked");
                }
            }
        }
    }

    // Summary
    let failed: Vec<&Task> = queue
        .all_tasks()
        .into_iter()
        .filter(|t| t.status == TaskStatus::Failed)
        .collect();
    let success = failed.is_empty();

    events.emit(EventKind::WorkflowComplete { success });

    if !success {
        eprintln!("\nWorkflow completed with failures:");
        for task in &failed {
            eprintln!(
                "  - {} ({}): {}",
                task.title,
                task.id,
                task.error.as_deref().unwrap_or("unknown error")
            );
        }
    } else {
        println!("\nWorkflow completed successfully.");
    }

    let results: serde_json::Map<String, JsonValue> = queue
        .all_tasks()
        .iter()
        .map(|t| {
            (
                t.title.clone(),
                serde_json::json!({
                    "status": format!("{:?}", t.status),
                    "result": t.result,
                    "error": t.error,
                }),
            )
        })
        .collect();

    Ok(serde_json::json!({
        "success": success,
        "tasks": results,
        "memory": memory.to_json(),
    }))
}

// ── Core Agent Execution ────────────────────────────────────────────────────

async fn run_agent(
    agent_root: &Path,
    entry_path: &Path,
    runtime_cwd: &Path,
    config: &mut AgentConfig,
    args_vec: Vec<String>,
    logger: Option<RunLogger>,
    memory: &SharedMemory,
    messages: &MessageBus,
) -> anyhow::Result<serde_json::Value> {
    if !entry_path.exists() {
        return Err(anyhow::anyhow!(
            "Entry point not found: {}",
            entry_path.display()
        ));
    }

    let agent_name = log_agent_name(config, agent_root);

    if let Some(l) = &logger {
        let args_json = serde_json::to_string(&args_vec).unwrap_or_default();
        l.log_line(format!(
            "agent.run.start name={} entry={} args_len={} args={}",
            agent_name,
            entry_path.display(),
            args_json.len(),
            args_json
        ));
    }

    enforce_hugind_version(config)?;

    // Auto-create a fresh session for agentic mode if none is declared
    if config.mode == crate::core::config::agent::AgentMode::Agentic
        && config.runtime_session.is_none()
    {
        config.runtime_session = Some(crate::core::config::agent::RuntimeSession {
            mode: crate::core::config::agent::SessionMode::Fresh,
            id: Some(format!("agentic-{}", uuid::Uuid::new_v4())),
        });
    }

    let backend = prepare_backend(config)?;

    check_server_health(&backend.health_url).await?;

    // Build initial data with team context
    let msg_context = messages.format_for_prompt(&agent_name);
    let mem_summary = memory.summary();

    let initial_data = serde_json::json!({
        "args": args_vec,
        "meta": {
            "session": config.runtime_session.clone(),
            "env": resolve_runtime_env(config)?,
        },
        "team": {
            "memory": memory.to_json(),
            "memory_summary": mem_summary,
            "messages": msg_context,
        }
    });

    let team_ctx = TeamContext::new(&agent_name, memory.clone(), messages.clone());

    let run_result = if entry_path.extension().and_then(|s| s.to_str()) == Some("wasm") {
        let wasm = WasmRuntime::new(
            agent_root.to_path_buf(),
            runtime_cwd.to_path_buf(),
            config,
            logger.clone(),
        )
        .map_err(|e| anyhow::anyhow!("Runtime error: {}", e))?;
        wasm.run_module_with_team(entry_path, initial_data, Some(&team_ctx), None)
            .await
            .map_err(|e| anyhow::anyhow!("Execution error: {}", e))
    } else if config.mode == crate::core::config::agent::AgentMode::Agentic {
        use crate::core::orchestrator::agentic::ToolRegistry;
        use crate::core::js::capabilities::agentic as agentic_cap;

        let trace = std::env::var("HUGIND_TRACE").map(|v| v == "1" || v == "true").unwrap_or(false);

        let registry = ToolRegistry::new();

        if trace { eprintln!("[trace] creating JS runtime"); }
        let js = JsRuntime::new_with_team(
            agent_root.to_path_buf(),
            runtime_cwd.to_path_buf(),
            config,
            logger.clone(),
            Some(&team_ctx),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Runtime error: {}", e))?;

        if trace { eprintln!("[trace] installing agentic globals"); }
        agentic_cap::install(&js.context(), &registry, logger.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to install agentic globals: {}", e))?;

        if trace { eprintln!("[trace] running entry point"); }
        let _ = js
            .run_module(entry_path, initial_data)
            .await
            .map_err(|e| anyhow::anyhow!("Agent setup error: {}", e))?;
        js.wait_idle().await;

        let user_prompt = build_agentic_prompt(&args_vec, &msg_context, &mem_summary);
        if trace { eprintln!("[trace] entry done, {} tools, prompt_len={}", registry.tools().len(), user_prompt.len()); }

        let output = run_agentic_loop_with_js(
            config,
            &backend.base_url,
            backend.model.as_deref().unwrap_or("default"),
            &user_prompt,
            &registry,
            &js,
            logger.as_ref(),
            trace,
        )
        .await?;

        Ok(serde_json::Value::String(output))
    } else {
        let js = JsRuntime::new_with_team(
            agent_root.to_path_buf(),
            runtime_cwd.to_path_buf(),
            config,
            logger.clone(),
            Some(&team_ctx),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Runtime error: {}", e))?;
        let res = js
            .run_module(entry_path, initial_data)
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

    cleanup_fresh_session(&backend, config).await;
    run_result
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Run the agentic LLM → tool → LLM loop using prompt-based tool calling.
///
/// The LLM receives tool descriptions in the system prompt (not as an API parameter).
/// It outputs tool calls as `<tool_call>{"name":"...","args":{...}}</tool_call>` in its text.
/// We parse these, execute them locally via the JS context, and send results back.
async fn run_agentic_loop_with_js(
    config: &AgentConfig,
    backend_url: &str,
    backend_model: &str,
    user_prompt: &str,
    registry: &crate::core::orchestrator::agentic::ToolRegistry,
    js: &JsRuntime,
    logger: Option<&RunLogger>,
    trace: bool,
) -> anyhow::Result<String> {
    use crate::core::orchestrator::agentic::{parse_tool_calls, strip_tool_calls};

    let url = format!("{}/chat/completions", backend_url.trim_end_matches('/'));
    let model = backend_model.to_string();
    // JS set_max_turns() overrides YAML max_turns which overrides default of 10
    let max_turns = registry
        .get_max_turns()
        .or(config.max_turns)
        .unwrap_or(10) as usize;

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    // Load installed skills and build catalog
    let installed_skills = crate::core::skill::load_all_skills().unwrap_or_default();
    let skill_catalog = crate::core::skill::build_skill_catalog(&installed_skills);

    // Build system prompt with skill catalog and tool descriptions embedded
    let mut system = registry.get_system_prompt().unwrap_or_default();
    if !skill_catalog.is_empty() {
        system.push_str(&skill_catalog);
    }
    let mut tools_section = registry.tools_prompt();
    if !installed_skills.is_empty() {
        // Inject activate_skill as a built-in tool alongside agent-registered tools
        if tools_section.is_empty() {
            tools_section = String::from(
                "\n\nYou have tools. To use one: <tool_call>{\"name\":\"tool_name\",\"args\":{...}}</tool_call>\n\
                 When done, respond without tool_call tags.\n\n",
            );
        }
        tools_section.push_str("- activate_skill(name): Load a skill's full instructions into context. Use this before starting work that matches a listed skill.\n");
    }
    if !tools_section.is_empty() {
        system.push_str(&tools_section);
    }

    let mut messages: Vec<serde_json::Value> = Vec::new();
    if !system.is_empty() {
        messages.push(serde_json::json!({"role": "system", "content": system}));
    }
    messages.push(serde_json::json!({"role": "user", "content": user_prompt}));

    let mut final_text = String::new();

    if trace { eprintln!("[trace] loop: url={} model={} max_turns={}", url, model, max_turns); }

    for turn in 0..max_turns {
        if trace { eprintln!("[trace] turn {}/{}: sending {} messages", turn, max_turns, messages.len()); }
        if let Some(l) = logger {
            l.log_line(format!("agentic.turn turn={} messages={}", turn, messages.len()));
        }

        let body = serde_json::json!({
            "model": model,
            "messages": messages,
            "stream": false,
        });

        let mut request = client.post(&url).json(&body);
        if let Some(session) = config.runtime_session.as_ref().and_then(|s| s.id.as_deref()) {
            request = request.header("X-Session-ID", session);
        }

        let llm_start = std::time::Instant::now();
        let resp = request.send().await
            .map_err(|e| anyhow::anyhow!("LLM request failed: {}", e))?;
        if trace { eprintln!("[trace] turn {}: response {} in {:.1}s", turn, resp.status(), llm_start.elapsed().as_secs_f64()); }
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("LLM error: {} {}", status, text));
        }

        let resp_json: serde_json::Value = resp.json().await?;
        let choice = resp_json.get("choices").and_then(|c| c.get(0))
            .ok_or_else(|| anyhow::anyhow!("No choices in LLM response"))?;
        let message = choice.get("message")
            .ok_or_else(|| anyhow::anyhow!("No message in choice"))?;
        let content = message.get("content").and_then(|c| c.as_str()).unwrap_or("");
        if trace { eprintln!("[trace] turn {}: content_len={} first_150={}", turn, content.len(), &content[..content.len().min(150)]); }

        messages.push(serde_json::json!({"role": "assistant", "content": content}));

        let tool_calls = parse_tool_calls(content);
        if trace { eprintln!("[trace] turn {}: {} tool calls", turn, tool_calls.len()); }

        if tool_calls.is_empty() {
            final_text = strip_tool_calls(content);
            if let Some(l) = logger {
                l.log_line(format!("agentic.complete turns={} final_len={}", turn + 1, final_text.len()));
            }
            return Ok(final_text);
        }

        let mut results = Vec::new();
        for tc in &tool_calls {
            let args_str = serde_json::to_string(&tc.args).unwrap_or_default();
            if trace { eprintln!("[trace] turn {}: exec {} args={}", turn, tc.name, &args_str[..args_str.len().min(80)]); }
            if let Some(l) = logger {
                l.log_line(format!("agentic.tool_call name={} args_len={}", tc.name, args_str.len()));
            }

            let tool_start = std::time::Instant::now();

            // Handle built-in activate_skill tool
            let result_str = if tc.name == "activate_skill" {
                let skill_name = tc.args.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match crate::core::skill::get_skill_instructions(skill_name) {
                    Ok(instructions) => {
                        if trace { eprintln!("[trace] turn {}: activated skill '{}' len={}", turn, skill_name, instructions.len()); }
                        instructions
                    }
                    Err(e) => format!("Error: skill '{}' not found: {}", skill_name, e),
                }
            } else {
                let result = execute_js_tool(js, &tc.name, &args_str).await;
                match &result {
                    Ok(r) => r.clone(),
                    Err(e) => format!("Error: {}", e),
                }
            };

            if trace { eprintln!("[trace] turn {}: {} done in {:.1}s len={}", turn, tc.name, tool_start.elapsed().as_secs_f64(), result_str.len()); }

            if let Some(l) = logger {
                l.log_line(format!("agentic.tool_result name={} result_len={}", tc.name, result_str.len()));
            }

            results.push(format!("[{}] {}", tc.name, result_str));
        }

        let results_msg = results.join("\n\n");
        messages.push(serde_json::json!({
            "role": "user",
            "content": format!("Tool results:\n\n{}", results_msg),
        }));
    }

    // Max turns reached — ask LLM for a final summary
    if let Some(l) = logger {
        l.log_line(format!("agentic.max_turns_reached max={}", max_turns));
    }

    messages.push(serde_json::json!({
        "role": "user",
        "content": "You have reached the maximum number of turns. Please provide your final answer now based on what you've found so far. Do not use any more tools."
    }));

    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": false,
    });

    let mut request = client.post(&url).json(&body);
    if let Some(session) = config.runtime_session.as_ref().and_then(|s| s.id.as_deref()) {
        request = request.header("X-Session-ID", session);
    }

    if let Ok(resp) = request.send().await {
        if resp.status().is_success() {
            if let Ok(resp_json) = resp.json::<serde_json::Value>().await {
                if let Some(content) = resp_json
                    .get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("message"))
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str())
                {
                    final_text = crate::core::orchestrator::agentic::strip_tool_calls(content);
                }
            }
        }
    }

    Ok(final_text)
}

/// Execute a tool by calling its JS function in __tool_executors.
/// Public wrapper for use from cli/agent.rs team command.
pub async fn execute_js_tool_pub(
    js: &JsRuntime,
    tool_name: &str,
    args_json: &str,
) -> anyhow::Result<String> {
    execute_js_tool(js, tool_name, args_json).await
}

async fn execute_js_tool(
    js: &JsRuntime,
    tool_name: &str,
    args_json: &str,
) -> anyhow::Result<String> {
    let tool_name = tool_name.to_string();
    let args_json = args_json.to_string();
    let result = std::sync::Arc::new(std::sync::Mutex::new(
        None::<anyhow::Result<String>>,
    ));
    let result_clone = result.clone();

    // Call the tool execute function via a JS wrapper that handles Promise resolution.
    // We set __tool_call_result after the execute function (or its Promise) resolves.
    js.context()
        .with(|ctx| {
            // Store args for the eval to pick up
            if let Ok(s) = rquickjs::String::from_str(ctx.clone(), &tool_name) {
                let _ = ctx.globals().set("__tc_name", s);
            }
            if let Ok(s) = rquickjs::String::from_str(ctx.clone(), &args_json) {
                let _ = ctx.globals().set("__tc_args", s);
            }
            let _ = ctx.globals().set("__tc_done", false);
            let _ = ctx.globals().set::<_, rquickjs::Value>("__tc_result",
                rquickjs::Value::new_null(ctx.clone()));

            // Eval a script that calls the tool and resolves any promise
            let script = r#"
                (async function() {
                    try {
                        var fn = __tool_executors[__tc_name];
                        if (!fn) {
                            __tc_result = "Error: tool '" + __tc_name + "' not found";
                            __tc_done = true;
                            return;
                        }
                        var result = fn(__tc_args);
                        if (result && typeof result.then === 'function') {
                            result = await result;
                        }
                        if (result === undefined || result === null) {
                            __tc_result = "OK";
                        } else if (typeof result === 'string') {
                            __tc_result = result;
                        } else {
                            __tc_result = JSON.stringify(result);
                        }
                    } catch(e) {
                        __tc_result = "Error: " + (e.message || e);
                    }
                    __tc_done = true;
                })();
            "#;
            let _ = ctx.eval::<rquickjs::Value, _>(script);
        })
        .await;

    // Drive the event loop until __tc_done is true
    for _ in 0..600 {
        js.wait_idle().await;

        let mut done = false;
        js.context()
            .with(|ctx| {
                if let Ok(d) = ctx.globals().get::<_, bool>("__tc_done") {
                    done = d;
                }
            })
            .await;

        if done {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    // Read the result
    js.context()
        .with(|ctx| {
            match ctx.globals().get::<_, rquickjs::Value>("__tc_result") {
                Ok(val) => {
                    let s = if let Some(js_str) = val.as_string() {
                        js_str.to_string().unwrap_or_default()
                    } else if val.is_null() || val.is_undefined() {
                        "OK".to_string()
                    } else {
                        format!("{:?}", val)
                    };
                    *result_clone.lock().unwrap() = Some(Ok(s));
                }
                Err(e) => {
                    *result_clone.lock().unwrap() =
                        Some(Err(anyhow::anyhow!("Failed to read tool result: {}", e)));
                }
            }
        })
        .await;

    result.lock().unwrap().take().unwrap_or_else(|| {
        Err(anyhow::anyhow!("Tool '{}' produced no result", tool_name))
    })
}

fn build_agentic_prompt(args: &[String], msg_context: &str, mem_summary: &str) -> String {
    let mut prompt = String::new();

    // Add team context if available
    if !mem_summary.is_empty() {
        prompt.push_str(mem_summary);
        prompt.push_str("\n\n");
    }
    if !msg_context.is_empty() {
        prompt.push_str(msg_context);
        prompt.push_str("\n\n");
    }

    // Add args as the main prompt
    // Look for --task-description or --prompt, otherwise join all args
    let mut i = 0;
    let mut description = None;
    while i < args.len() {
        if (args[i] == "--task-description" || args[i] == "--prompt") && i + 1 < args.len() {
            description = Some(args[i + 1].clone());
            break;
        }
        i += 1;
    }

    if let Some(desc) = description {
        prompt.push_str(&desc);
    } else if !args.is_empty() {
        prompt.push_str(&args.join(" "));
    }

    prompt
}

async fn check_server_health(health_url: &str) -> anyhow::Result<()> {
    println!("Checking server health at {}...", health_url);
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(3))
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    if let Err(e) = client
        .get(health_url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
    {
        return Err(anyhow::anyhow!(
            "Server not reachable at {}: {}",
            health_url,
            e
        ));
    }
    println!("Server is up. Starting agent...");
    Ok(())
}

async fn cleanup_fresh_session(backend: &ResolvedBackend, config: &AgentConfig) {
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

fn enforce_hugind_version(config: &AgentConfig) -> anyhow::Result<()> {
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
    config: &AgentConfig,
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

fn log_agent_name(config: &AgentConfig, agent_root: &Path) -> String {
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
    config: &AgentConfig,
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

fn resolve_runtime_env(config: &AgentConfig) -> anyhow::Result<JsonMap<String, JsonValue>> {
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
        let value = serde_yaml::from_str::<YamlValue>("name: TOKEN\nrequired: true\n")
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
