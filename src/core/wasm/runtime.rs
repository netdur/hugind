use crate::core::config::agent::{AgentConfig, NetPermissions, RuntimeFsMode, ShellPermission};
use crate::core::config::backend::resolve_backend;
use crate::core::fs::FsAccess;
use crate::core::mcp::McpManager;
use crate::core::orchestrator::agentic::{AgentTool, ToolRegistry};
use crate::core::orchestrator::context::TeamContext;
use crate::core::orchestrator::task::Task;
use crate::core::runtime::util::{parse_duration_string, parse_memory_string};
use crate::shared::logging::RunLogger;
use anyhow::{Context, Result, anyhow, bail};
use cap_std::fs::Dir as CapDir;
use futures::StreamExt;
use reqwest::Url;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use wasmtime::component::ResourceTable;
use wasmtime::{AsContextMut, Caller, Config, Engine, Linker, Module, Store};
use wasmtime_wasi::preview2::preview1::{WasiPreview1Adapter, WasiPreview1View};
use wasmtime_wasi::preview2::{DirPerms, FilePerms, WasiCtx, WasiCtxBuilder, WasiView};

struct HostState {
    args_json: String,
    result_json: Option<serde_json::Value>,
    net_permission: NetPermissions,
    shell_permission: Option<ShellPermission>,
    fs_access: FsAccess,
    fs_mode: RuntimeFsMode,
    net_client: reqwest::Client,
    llm_client: reqwest::Client,
    llm_base_url: String,
    llm_model: Option<String>,
    hugind_version: String,
    llm_session_id: Option<String>,
    wasi: WasiCtx,
    table: ResourceTable,
    adapter: WasiPreview1Adapter,
    limits: wasmtime::StoreLimits,
    logger: Option<RunLogger>,
    team_ctx: Option<TeamContext>,
    tool_registry: Option<ToolRegistry>,
    skill_catalog: String,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl WasiPreview1View for HostState {
    fn adapter(&self) -> &WasiPreview1Adapter {
        &self.adapter
    }
    fn adapter_mut(&mut self) -> &mut WasiPreview1Adapter {
        &mut self.adapter
    }
}

pub struct WasmRuntime {
    engine: Engine,
    agent_root: PathBuf,
    fs_root: PathBuf,
    config: AgentConfig,
    logger: Option<RunLogger>,
}

impl WasmRuntime {
    pub fn new(
        agent_root: PathBuf,
        fs_root: PathBuf,
        config: &AgentConfig,
        logger: Option<RunLogger>,
    ) -> Result<Self> {
        let mut wasm_config = Config::new();
        wasm_config.async_support(true);

        if let Some(wasm_opts) = &config.wasm {
            if let Some(resources) = &wasm_opts.resources {
                if resources.cpu.is_some() {
                    wasm_config.consume_fuel(true);
                }

                // Memory limits applied via StoreLimitsBuilder below
            }
        }

        let engine = Engine::new(&wasm_config)?;
        Ok(Self {
            engine,
            agent_root,
            fs_root,
            config: config.clone(),
            logger,
        })
    }

    pub async fn run_module(
        &self,
        entry: &Path,
        args_val: serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.run_module_inner(entry, args_val, None, None).await
    }

    pub async fn run_module_with_team(
        &self,
        entry: &Path,
        args_val: serde_json::Value,
        team_ctx: Option<&TeamContext>,
        tool_registry: Option<&ToolRegistry>,
    ) -> Result<serde_json::Value> {
        self.run_module_inner(entry, args_val, team_ctx, tool_registry).await
    }

    async fn run_module_inner(
        &self,
        entry: &Path,
        args_val: serde_json::Value,
        team_ctx: Option<&TeamContext>,
        tool_registry: Option<&ToolRegistry>,
    ) -> Result<serde_json::Value> {
        let entry = entry
            .canonicalize()
            .map_err(|e| anyhow!("entry not found: {}", e))?;

        if !entry.starts_with(&self.agent_root) {
            bail!("entry escapes agent root");
        }

        let args_json = serde_json::to_string(&args_val)
            .map_err(|e| anyhow!("failed to serialize args: {}", e))?;

        let resolved = resolve_backend(&self.config)?;
        let llm_base_url = resolved.base_url;
        let llm_model = resolved.model;
        let hugind_version = env!("CARGO_PKG_VERSION").to_string();
        let llm_session_id = resolved.session.as_ref().and_then(|s| s.id.clone());

        let net_permission = if let Some(p) = &self.config.permissions {
            p.network.clone().unwrap_or_default()
        } else {
            NetPermissions::default()
        };

        let shell_permission = if let Some(p) = &self.config.permissions {
            p.shell.clone()
        } else {
            None
        };

        let net_client = reqwest::Client::builder()
            .user_agent("Hugind/0.1 (http://github.com/netdur/hugind)")
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| anyhow!("client build error: {}", e))?;

        let fs_mode = self
            .config
            .wasm
            .as_ref()
            .map(|w| w.runtime_fs_mode.clone())
            .unwrap_or_default();

        let fs_access = FsAccess::new(
            self.fs_root.clone(),
            self.config
                .permissions
                .as_ref()
                .and_then(|p| p.filesystem.clone()),
        );

        let mut wasi_builder = WasiCtxBuilder::new();
        wasi_builder.inherit_stdio();

        let mut enable_wasi_mounts = true;
        if let Some(wasm_opts) = &self.config.wasm {
            use crate::core::config::agent::RuntimeFsMode;
            if wasm_opts.runtime_fs_mode == RuntimeFsMode::HostFilesystem {
                enable_wasi_mounts = false;
            }
        }

        if enable_wasi_mounts {
            if let Some(wasm_opts) = &self.config.wasm {
                if let Some(mounts) = &wasm_opts.mounts {
                    let allow_outside_agent_root = self
                        .config
                        .permissions
                        .as_ref()
                        .and_then(|p| p.filesystem.as_ref())
                        .map(|p| p.allow_outside_agent_root)
                        .unwrap_or(false);

                    for mount in mounts {
                        let host_path =
                            Path::new(&mount.host).canonicalize().with_context(|| {
                                format!("failed to canonicalize host path: {}", mount.host)
                            })?;

                        let guest_path = Path::new(&mount.guest);
                        if !guest_path.is_absolute() {
                            bail!("mount guest path must be absolute: {}", mount.guest);
                        }
                        if guest_path
                            .components()
                            .any(|c| matches!(c, std::path::Component::ParentDir))
                        {
                            bail!("mount guest path must not contain '..': {}", mount.guest);
                        }

                        if !allow_outside_agent_root && !host_path.starts_with(&self.agent_root) {
                            bail!(
                                "mount '{}' is outside agent root; set permissions.filesystem.allow_outside_agent_root=true to allow",
                                host_path.display()
                            );
                        }

                        let dir =
                            CapDir::open_ambient_dir(&host_path, cap_std::ambient_authority())
                                .with_context(|| {
                                    format!("failed to open host dir: {:?}", host_path)
                                })?;

                        wasi_builder.preopened_dir(
                            dir,
                            DirPerms::all(),
                            FilePerms::all(),
                            &mount.guest,
                        );
                    }
                }
            }
        }

        let wasi = wasi_builder.build();
        let table = ResourceTable::new();
        let adapter = WasiPreview1Adapter::new();

        let mut limits_builder = wasmtime::StoreLimitsBuilder::new();
        if let Some(wasm_opts) = &self.config.wasm {
            if let Some(res) = &wasm_opts.resources {
                if let Some(mem_str) = &res.memory {
                    if let Some(bytes) = parse_memory_string(mem_str) {
                        limits_builder = limits_builder.memory_size(bytes);
                    }
                }
            }
        }
        let limits = limits_builder.build();

        let mut store = Store::new(
            &self.engine,
            HostState {
                args_json,
                result_json: None,
                net_permission,
                shell_permission,
                fs_access,
                fs_mode,
                net_client,
                llm_client: reqwest::Client::new(),
                llm_base_url,
                llm_model,
                hugind_version,
                llm_session_id,
                wasi,
                table,
                adapter,
                limits,
                logger: self.logger.clone(),
                team_ctx: team_ctx.cloned(),
                tool_registry: tool_registry.cloned(),
                skill_catalog: {
                    let skills = crate::core::skill::load_all_skills().unwrap_or_default();
                    crate::core::skill::build_skill_catalog(&skills)
                },
            },
        );

        store.limiter(|s| &mut s.limits);

        let mut global_timeout = None;
        if let Some(wasm_opts) = &self.config.wasm {
            if let Some(res) = &wasm_opts.resources {
                global_timeout = res.timeout.as_deref().and_then(parse_duration_string);
            }
        }

        if let Some(wasm_opts) = &self.config.wasm {
            if let Some(res) = &wasm_opts.resources {
                if let Some(cpu_str) = &res.cpu {
                    // Parse CPU budget as instruction count (e.g., "1000000" or "1M")
                    let fuel = parse_memory_string(cpu_str).unwrap_or(1_000_000_000);
                    store.set_fuel(fuel as u64).ok();
                }
            }
        }

        let module = Module::from_file(&self.engine, &entry)
            .with_context(|| format!("failed to load wasm module: {}", entry.display()))?;

        let mcp_manager = McpManager::new(&self.config)
            .await
            .map_err(|e| anyhow!("MCP Error: {}", e))?
            .map(Arc::new);

        let mut linker = Linker::new(&self.engine);

        wasmtime_wasi::preview2::preview1::add_to_linker_async(&mut linker)?;

        Self::link_host_functions(&mut linker, mcp_manager)?;

        let instance = linker
            .instantiate_async(&mut store, &module)
            .await
            .map_err(|e| anyhow!("failed to instantiate wasm module: {}", e))?;

        let run_future = async {
            if let Ok(start) = instance.get_typed_func::<(), ()>(&mut store, "_start") {
                start.call_async(&mut store, ()).await?;
            } else if let Ok(main) = instance.get_typed_func::<(), ()>(&mut store, "main") {
                main.call_async(&mut store, ()).await?;
            }
            Ok::<(), anyhow::Error>(())
        };

        if let Some(timeout_duration) = global_timeout {
            if let Err(_) = tokio::time::timeout(timeout_duration, run_future).await {
                bail!("Wasm execution timed out after {:?}", timeout_duration);
            }
        } else {
            run_future.await?;
        }

        let result = store
            .data()
            .result_json
            .clone()
            .unwrap_or(serde_json::Value::Null);

        Ok(result)
    }

    fn link_host_functions(
        linker: &mut Linker<HostState>,
        mcp_manager: Option<Arc<McpManager>>,
    ) -> Result<()> {
        linker.func_wrap(
            "env",
            "abort",
            |caller: Caller<'_, HostState>,
             _msg: i32,
             _file: i32,
             line: i32,
             col: i32|
             -> Result<()> {
                log_host(&caller, format!("host.sys.abort line={} col={}", line, col));
                eprintln!("Guest Error: abort() called at line {}:{}", line, col);
                Err(anyhow::anyhow!("Guest execution aborted"))
            },
        )?;

        linker.func_wrap(
            "hugind",
            "print",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                let msg = read_string(&mut caller, ptr, len)?;
                crate::shared::stdio::print(&msg);
                Ok(())
            },
        )?;

        linker.func_wrap(
            "hugind",
            "print_raw",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                let msg = read_string(&mut caller, ptr, len)?;
                crate::shared::stdio::print_raw(&msg);
                Ok(())
            },
        )?;

        linker.func_wrap2_async(
            "hugind",
            "input",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                Box::new(async move {
                    let prompt = read_string(&mut caller, ptr, len)?;
                    log_host(
                        &caller,
                        format!("host.sys.input prompt_len={}", prompt.len()),
                    );
                    let mut stdout = io::stdout();
                    let _ = stdout.write_all(prompt.as_bytes()).await;
                    let _ = stdout.flush().await;

                    let mut reader = BufReader::new(io::stdin());
                    let mut buffer = String::new();
                    let _ = reader.read_line(&mut buffer).await;
                    let value = buffer.trim().to_string();
                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, value.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        linker.func_wrap2_async(
            "hugind",
            "run_command",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                Box::new(async move {
                    let cmd_str = read_string(&mut caller, ptr, len)?;
                    log_host(&caller, format!("host.shell.run_command cmd={}", cmd_str));

                    let perm = caller.data().shell_permission.clone().unwrap_or_default();
                    ensure_shell_command_allowed(&perm)?;

                    // We don't check whitelist/blacklist here anymore as it breaks when wrapped in `sh -c`.
                    // The WASM environment is designed to be an agent running bash commands.

                    let mut command = if cfg!(target_os = "macos") {
                        let profile = crate::core::runtime::sandbox::macos_sandbox_profile(&perm);
                        let mut cmd = Command::new("sandbox-exec");
                        cmd.arg("-p").arg(profile).arg("sh").arg("-c").arg(&cmd_str);
                        cmd
                    } else {
                        let mut cmd = Command::new("sh");
                        cmd.arg("-c").arg(&cmd_str);
                        cmd
                    };

                    if perm.env_clear {
                        command.env_clear();
                    }
                    if let Some(wd) = &perm.working_dir {
                        command.current_dir(wd);
                    }

                    let timeout = perm.timeout.as_deref().and_then(parse_duration_string);

                    let output_fut = command.output();
                    let output_res = if let Some(t) = timeout {
                        match tokio::time::timeout(t, output_fut).await {
                            Ok(res) => res,
                            Err(_) => bail!("Shell command timed out"),
                        }
                    } else {
                        output_fut.await
                    };

                    let output =
                        output_res.map_err(|e| anyhow!("Failed to execute command: {}", e))?;

                    let max_len = perm
                        .max_output
                        .as_deref()
                        .and_then(parse_memory_string)
                        .unwrap_or(1024 * 1024);

                    let result_str = format_run_command_output(
                        output.status.success(),
                        &output.stdout,
                        &output.stderr,
                        max_len,
                    );

                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, result_str.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        linker.func_wrap2_async(
            "hugind",
            "spawn",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                Box::new(async move {
                    let json_args = read_string(&mut caller, ptr, len)?;
                    let args_vec: Vec<String> = serde_json::from_str(&json_args).map_err(|e| {
                        anyhow!("spawn arguments must be a JSON array of strings: {}", e)
                    })?;

                    if args_vec.is_empty() {
                        bail!("Spawn: empty arguments");
                    }

                    let program = &args_vec[0];
                    let args = &args_vec[1..];

                    log_host(
                        &caller,
                        format!("host.shell.spawn program={} args={:?}", program, args),
                    );

                    let perm = caller.data().shell_permission.clone().unwrap_or_default();
                    ensure_spawn_program_allowed(program, &perm)?;

                    let mut command = if cfg!(target_os = "macos") {
                        let profile = crate::core::runtime::sandbox::macos_sandbox_profile(&perm);
                        let mut cmd = Command::new("sandbox-exec");
                        cmd.arg("-p").arg(profile).arg(program);
                        cmd
                    } else {
                        Command::new(program)
                    };

                    command.args(args);

                    if perm.env_clear {
                        command.env_clear();
                    }
                    if let Some(wd) = &perm.working_dir {
                        command.current_dir(wd);
                    }

                    let timeout = perm.timeout.as_deref().and_then(parse_duration_string);

                    let output_fut = command.output();
                    let output_res = if let Some(t) = timeout {
                        match tokio::time::timeout(t, output_fut).await {
                            Ok(res) => res,
                            Err(_) => bail!("Shell command timed out"),
                        }
                    } else {
                        output_fut.await
                    };

                    let output =
                        output_res.map_err(|e| anyhow!("Failed to execute command: {}", e))?;

                    let max_len = perm
                        .max_output
                        .as_deref()
                        .and_then(parse_memory_string)
                        .unwrap_or(1024 * 1024);

                    let result_str = format_spawn_output(
                        output.status.success(),
                        &output.stdout,
                        &output.stderr,
                        max_len,
                    );

                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, result_str.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        linker.func_wrap2_async(
            "hugind",
            "net_fetch",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                Box::new(async move {
                    let url = read_string(&mut caller, ptr, len)?;
                    log_host(&caller, format!("host.net.fetch url={}", url));
                    let permission = caller.data().net_permission.clone();
                    let client = caller.data().net_client.clone();

                    let mut current =
                        Url::parse(&url).map_err(|e| anyhow!("Invalid URL: {}", e))?;
                    let max_redirects = 5;

                    for _ in 0..=max_redirects {
                        let (host, port) = validate_net_fetch_target(&current, &permission)?;

                        let ips: Vec<std::net::IpAddr> = if let Ok(ip) =
                            host.parse::<std::net::IpAddr>()
                        {
                            vec![ip]
                        } else {
                            let addr_str = format!("{}:{}", host, port);
                            tokio::net::lookup_host(&addr_str)
                                .await
                                .map_err(|e| anyhow!("DNS resolution failed for {}: {}", host, e))?
                                .map(|sa| sa.ip())
                                .collect()
                        };
                        ensure_public_network_access(&permission, &ips)?;

                        let timeout_duration = permission
                            .timeout
                            .as_deref()
                            .and_then(parse_duration_string)
                            .unwrap_or(std::time::Duration::from_secs(30));

                        let max_bytes = permission
                            .max_response_bytes
                            .as_deref()
                            .and_then(parse_memory_string)
                            .unwrap_or(10 * 1024 * 1024);

                        let res = client
                            .get(current.clone())
                            .timeout(timeout_duration)
                            .send()
                            .await
                            .map_err(|e| anyhow!("Network Request Failed: {}", e))?;

                        if res.status().is_redirection() {
                            let location = res
                                .headers()
                                .get(reqwest::header::LOCATION)
                                .ok_or_else(|| anyhow!("Redirect missing Location header"))?
                                .to_str()
                                .map_err(|e| anyhow!("Invalid Location header: {}", e))?;
                            current = current
                                .join(location)
                                .map_err(|e| anyhow!("Invalid redirect URL: {}", e))?;
                            continue;
                        }

                        if !res.status().is_success() {
                            bail!("HTTP Status: {}", res.status());
                        }

                        let mut content = Vec::new();
                        let mut stream = res.bytes_stream();

                        while let Some(item) = stream.next().await {
                            let chunk = item.map_err(|e| anyhow!("Chunk error: {}", e))?;
                            if content.len() + chunk.len() > max_bytes {
                                let remaining = max_bytes - content.len();
                                content.extend_from_slice(&chunk[..remaining]);
                                break;
                            }
                            content.extend_from_slice(&chunk);
                        }

                        let text = String::from_utf8_lossy(&content).to_string();

                        let (out_ptr, out_len) =
                            write_bytes_async(&mut caller, text.as_bytes()).await?;
                        return Ok(pack_ptr_len(out_ptr, out_len));
                    }

                    bail!("Too many redirects");
                })
            },
        )?;

        linker.func_wrap2_async(
            "hugind",
            "llm_chat",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                Box::new(async move {
                    let input = read_string(&mut caller, ptr, len)?;
                    let base_url = caller.data().llm_base_url.clone();
                    let model = caller.data().llm_model.clone();
                    let client = caller.data().llm_client.clone();
                    let session_id = caller.data().llm_session_id.clone();

                    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
                    let (body, used_object_input) =
                        build_wasm_llm_body(&input, model.as_ref(), false, "llm_chat")?;
                    if used_object_input {
                        let msg_len = body
                            .get("messages")
                            .and_then(|v| v.as_array())
                            .map(|a| a.len());
                        let model_name = body.get("model").and_then(|v| v.as_str()).unwrap_or("");
                        log_host(
                            &caller,
                            format!(
                                "host.llm.chat input=object messages={:?} model={}",
                                msg_len, model_name
                            ),
                        );
                    } else {
                        log_host(
                            &caller,
                            format!("host.llm.chat input=string prompt_len={}", input.len()),
                        );
                    }

                    let mut request = client
                        .post(&url)
                        .json(&body);
                    if let Some(id) = session_id {
                        request = request.header("X-Session-ID", id);
                    }
                    let res = request
                        .send()
                        .await
                        .map_err(|e| anyhow!("LLM Request Failed: {}", e))?;

                    if !res.status().is_success() {
                        let status = res.status();

                        let bytes = res.bytes().await.unwrap_or_default();
                        let text = String::from_utf8_lossy(&bytes);
                        bail!("LLM Error: {}: {}", status, text);
                    }

                    let mut content = Vec::new();
                    let mut stream = res.bytes_stream();

                    while let Some(item) = stream.next().await {
                        let chunk = item.map_err(|e| anyhow!("LLM Chunk error: {}", e))?;
                        content.extend_from_slice(&chunk);
                    }

                    let body_text = String::from_utf8_lossy(&content).to_string();

                    let content = extract_llm_content(&body_text);
                    log_host(
                        &caller,
                        format!("host.llm.chat response_len={}", content.len()),
                    );
                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, content.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        linker.func_wrap2_async(
            "hugind",
            "llm_chat_stream",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                Box::new(async move {
                    let input = read_string(&mut caller, ptr, len)?;
                    let base_url = caller.data().llm_base_url.clone();
                    let model = caller.data().llm_model.clone();
                    let client = caller.data().llm_client.clone();
                    let session_id = caller.data().llm_session_id.clone();

                    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
                    let (body, used_object_input) =
                        build_wasm_llm_body(&input, model.as_ref(), true, "llm_chat_stream")?;
                    if used_object_input {
                        let msg_len = body
                            .get("messages")
                            .and_then(|v| v.as_array())
                            .map(|a| a.len());
                        let model_name = body.get("model").and_then(|v| v.as_str()).unwrap_or("");
                        log_host(
                            &caller,
                            format!(
                                "host.llm.chat_stream input=object messages={:?} model={}",
                                msg_len, model_name
                            ),
                        );
                    } else {
                        log_host(
                            &caller,
                            format!(
                                "host.llm.chat_stream input=string prompt_len={}",
                                input.len()
                            ),
                        );
                    }

                    let mut request = client
                        .post(&url)
                        .json(&body);
                    if let Some(id) = session_id {
                        request = request.header("X-Session-ID", id);
                    }
                    let res = request
                        .send()
                        .await
                        .map_err(|e| anyhow!("LLM Request Failed: {}", e))?;

                    if !res.status().is_success() {
                        let status = res.status();
                        let bytes = res.bytes().await.unwrap_or_default();
                        let text = String::from_utf8_lossy(&bytes);
                        bail!("LLM Error: {}: {}", status, text);
                    }

                    let mut content = String::new();
                    let mut stream = res.bytes_stream();
                    let mut sse_buffer = String::new();
                    let on_token = caller
                        .get_export("llm_on_token")
                        .and_then(|e| e.into_func())
                        .and_then(|f| f.typed::<(i32, i32), ()>(caller.as_context_mut()).ok());
                    let on_sse = caller
                        .get_export("llm_on_sse")
                        .and_then(|e| e.into_func())
                        .and_then(|f| f.typed::<(i32, i32), ()>(caller.as_context_mut()).ok());

                    while let Some(item) = stream.next().await {
                        let chunk = item.map_err(|e| anyhow!("LLM Chunk error: {}", e))?;
                        let text = String::from_utf8_lossy(&chunk);
                        sse_buffer.push_str(&text);
                        while let Some(newline_idx) = sse_buffer.find('\n') {
                            let mut line = sse_buffer[..newline_idx].to_string();
                            if line.ends_with('\r') {
                                line.pop();
                            }
                            sse_buffer = sse_buffer[newline_idx + 1..].to_string();
                            if let Some(cb) = &on_sse {
                                let (line_ptr, line_len) =
                                    write_bytes_async(&mut caller, line.as_bytes()).await?;
                                cb.call_async(
                                    caller.as_context_mut(),
                                    (line_ptr as i32, line_len as i32),
                                )
                                .await
                                .ok();
                            }
                            if !line.starts_with("data: ") {
                                continue;
                            }
                            let data_str = &line[6..];
                            if data_str.trim() == "[DONE]" {
                                continue;
                            }
                            if let Ok(data) = serde_json::from_str::<serde_json::Value>(data_str) {
                                if let Some(delta) = data
                                    .get("choices")
                                    .and_then(|choices| choices.get(0))
                                    .and_then(|choice| choice.get("delta"))
                                    .and_then(|delta| delta.get("content"))
                                    .and_then(|content| content.as_str())
                                {
                                    content.push_str(delta);
                                    if let Some(cb) = &on_token {
                                        let (out_ptr, out_len) =
                                            write_bytes_async(&mut caller, delta.as_bytes())
                                                .await?;
                                        cb.call_async(
                                            caller.as_context_mut(),
                                            (out_ptr as i32, out_len as i32),
                                        )
                                        .await
                                        .ok();
                                    }
                                }
                            }
                        }
                    }
                    let trailing = sse_buffer.trim_end_matches('\r').trim();
                    if !trailing.is_empty() {
                        if let Some(cb) = &on_sse {
                            let (line_ptr, line_len) =
                                write_bytes_async(&mut caller, trailing.as_bytes()).await?;
                            cb.call_async(
                                caller.as_context_mut(),
                                (line_ptr as i32, line_len as i32),
                            )
                            .await
                            .ok();
                        }
                    }
                    if trailing.starts_with("data: ") {
                        let data_str = &trailing[6..];
                        if data_str.trim() != "[DONE]" {
                            if let Ok(data) = serde_json::from_str::<serde_json::Value>(data_str) {
                                if let Some(delta) = data
                                    .get("choices")
                                    .and_then(|choices| choices.get(0))
                                    .and_then(|choice| choice.get("delta"))
                                    .and_then(|delta| delta.get("content"))
                                    .and_then(|content| content.as_str())
                                {
                                    content.push_str(delta);
                                    if let Some(cb) = &on_token {
                                        let (out_ptr, out_len) =
                                            write_bytes_async(&mut caller, delta.as_bytes())
                                                .await?;
                                        cb.call_async(
                                            caller.as_context_mut(),
                                            (out_ptr as i32, out_len as i32),
                                        )
                                        .await
                                        .ok();
                                    }
                                }
                            }
                        }
                    }

                    log_host(
                        &caller,
                        format!("host.llm.chat_stream response_len={}", content.len()),
                    );
                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, content.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        linker.func_wrap0_async("hugind", "get_args", |mut caller: Caller<'_, HostState>| {
            Box::new(async move {
                let args_json = caller.data().args_json.clone();
                log_host(&caller, "host.sys.get_args");
                let (out_ptr, out_len) =
                    write_bytes_async(&mut caller, args_json.as_bytes()).await?;
                Ok(pack_ptr_len(out_ptr, out_len))
            })
        })?;

        linker.func_wrap0_async("hugind", "version", |mut caller: Caller<'_, HostState>| {
            Box::new(async move {
                let version = caller.data().hugind_version.clone();
                let (out_ptr, out_len) = write_bytes_async(&mut caller, version.as_bytes()).await?;
                Ok(pack_ptr_len(out_ptr, out_len))
            })
        })?;

        linker.func_wrap(
            "hugind",
            "set_result",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                let json_str = read_string(&mut caller, ptr, len)?;
                log_host(
                    &caller,
                    format!("host.sys.set_result bytes={}", json_str.len()),
                );
                let parsed = serde_json::from_str::<serde_json::Value>(&json_str)
                    .map_err(|e| anyhow!("Invalid JSON result: {}", e))?;
                caller.data_mut().result_json = Some(parsed);
                Ok(())
            },
        )?;

        let tools_list_manager = mcp_manager.clone();
        linker.func_wrap0_async(
            "hugind",
            "tools_list",
            move |mut caller: Caller<'_, HostState>| {
                let tools_list_manager = tools_list_manager.clone();
                Box::new(async move {
                    log_host(&caller, "host.tools.list");
                    let tools = match &tools_list_manager {
                        Some(m) => m.list_tools().await?,
                        None => Vec::new(),
                    };
                    let json = serde_json::to_string(&tools)
                        .map_err(|e| anyhow!("failed to serialize tools list: {}", e))?;
                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, json.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        let tools_call_manager = mcp_manager.clone();
        linker.func_wrap2_async(
            "hugind",
            "tools_call",
            move |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                let tools_call_manager = tools_call_manager.clone();
                Box::new(async move {
                    let req_json = read_string(&mut caller, ptr, len)?;
                    let req: ToolsCallInput = serde_json::from_str(&req_json)
                        .map_err(|e| anyhow!("tools_call expects JSON {{name,args}}: {}", e))?;

                    let mut args = req.args;
                    if args.is_null() {
                        args = serde_json::Value::Object(serde_json::Map::new());
                    }
                    let args_len = serde_json::to_string(&args).map(|s| s.len()).unwrap_or(0);
                    log_host(
                        &caller,
                        format!("host.tools.call name={} args_len={}", req.name, args_len),
                    );

                    let manager = tools_call_manager
                        .as_ref()
                        .ok_or_else(|| anyhow!("No MCP tools configured"))?;
                    let result = manager.call_tool(&req.name, args).await?;
                    let json = serde_json::to_string(&result)
                        .map_err(|e| anyhow!("failed to serialize tool result: {}", e))?;
                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, json.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        Self::link_fs_hostcalls(linker)?;
        Self::link_team_hostcalls(linker)?;
        Self::link_agentic_hostcalls(linker)?;
        Self::link_skill_hostcalls(linker)?;

        Ok(())
    }

    fn link_team_hostcalls(linker: &mut Linker<HostState>) -> Result<()> {
        // -- memory.set(key, value) --
        linker.func_wrap(
            "hugind",
            "memory_set",
            |mut caller: Caller<'_, HostState>,
             key_ptr: i32,
             key_len: i32,
             val_ptr: i32,
             val_len: i32|
             -> Result<()> {
                let key = read_string(&mut caller, key_ptr, key_len)?;
                let val_str = read_string(&mut caller, val_ptr, val_len)?;
                let team = caller
                    .data()
                    .team_ctx
                    .as_ref()
                    .ok_or_else(|| anyhow!("memory.set requires team context"))?;
                let agent_name = team.agent_name.clone();
                let value: serde_json::Value = serde_json::from_str(&val_str)
                    .unwrap_or(serde_json::Value::String(val_str));
                log_host(&caller, format!("host.memory.set key={}", key));
                caller
                    .data()
                    .team_ctx
                    .as_ref()
                    .unwrap()
                    .memory
                    .set(&agent_name, &key, value);
                Ok(())
            },
        )?;

        // -- memory.get(key) -> json string --
        linker.func_wrap2_async(
            "hugind",
            "memory_get",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                Box::new(async move {
                    let key = read_string(&mut caller, ptr, len)?;
                    log_host(&caller, format!("host.memory.get key={}", key));
                    let team = caller
                        .data()
                        .team_ctx
                        .as_ref()
                        .ok_or_else(|| anyhow!("memory.get requires team context"))?;
                    let val = team.memory.get(&key);
                    let json = match val {
                        Some(v) => serde_json::to_string(&v).unwrap_or_else(|_| "null".into()),
                        None => "null".into(),
                    };
                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, json.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        // -- memory.list() -> json object --
        linker.func_wrap0_async(
            "hugind",
            "memory_list",
            |mut caller: Caller<'_, HostState>| {
                Box::new(async move {
                    log_host(&caller, "host.memory.list");
                    let team = caller
                        .data()
                        .team_ctx
                        .as_ref()
                        .ok_or_else(|| anyhow!("memory.list requires team context"))?;
                    let json_val = team.memory.to_json();
                    let json = serde_json::to_string(&json_val)
                        .unwrap_or_else(|_| "{}".into());
                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, json.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        // -- memory.summary() -> markdown string --
        linker.func_wrap0_async(
            "hugind",
            "memory_summary",
            |mut caller: Caller<'_, HostState>| {
                Box::new(async move {
                    log_host(&caller, "host.memory.summary");
                    let team = caller
                        .data()
                        .team_ctx
                        .as_ref()
                        .ok_or_else(|| anyhow!("memory.summary requires team context"))?;
                    let summary = team.memory.summary();
                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, summary.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        // -- messaging.send(to, content) --
        linker.func_wrap(
            "hugind",
            "messaging_send",
            |mut caller: Caller<'_, HostState>,
             to_ptr: i32,
             to_len: i32,
             content_ptr: i32,
             content_len: i32|
             -> Result<()> {
                let to = read_string(&mut caller, to_ptr, to_len)?;
                let content = read_string(&mut caller, content_ptr, content_len)?;
                let team = caller
                    .data()
                    .team_ctx
                    .as_ref()
                    .ok_or_else(|| anyhow!("messaging.send requires team context"))?;
                let from = team.agent_name.clone();
                log_host(
                    &caller,
                    format!("host.messaging.send from={} to={}", from, to),
                );
                caller
                    .data()
                    .team_ctx
                    .as_ref()
                    .unwrap()
                    .messages
                    .send(&from, &to, &content);
                Ok(())
            },
        )?;

        // -- messaging.broadcast(content) --
        linker.func_wrap(
            "hugind",
            "messaging_broadcast",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| -> Result<()> {
                let content = read_string(&mut caller, ptr, len)?;
                let team = caller
                    .data()
                    .team_ctx
                    .as_ref()
                    .ok_or_else(|| anyhow!("messaging.broadcast requires team context"))?;
                let from = team.agent_name.clone();
                log_host(
                    &caller,
                    format!("host.messaging.broadcast from={}", from),
                );
                caller
                    .data()
                    .team_ctx
                    .as_ref()
                    .unwrap()
                    .messages
                    .broadcast(&from, &content);
                Ok(())
            },
        )?;

        // -- messaging.receive() -> json array --
        linker.func_wrap0_async(
            "hugind",
            "messaging_receive",
            |mut caller: Caller<'_, HostState>| {
                Box::new(async move {
                    log_host(&caller, "host.messaging.receive");
                    let team = caller
                        .data()
                        .team_ctx
                        .as_ref()
                        .ok_or_else(|| anyhow!("messaging.receive requires team context"))?;
                    let agent_name = team.agent_name.clone();
                    let msgs = team.messages.receive(&agent_name);
                    let arr: Vec<serde_json::Value> = msgs
                        .iter()
                        .map(|m| {
                            serde_json::json!({
                                "from": m.from,
                                "to": m.to,
                                "content": m.content,
                            })
                        })
                        .collect();
                    let json = serde_json::to_string(&arr)
                        .unwrap_or_else(|_| "[]".into());
                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, json.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        // -- tasks.spawn(json) -> json result --
        linker.func_wrap2_async(
            "hugind",
            "tasks_spawn",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                Box::new(async move {
                    let spec_json = read_string(&mut caller, ptr, len)?;
                    log_host(&caller, "host.tasks.spawn");
                    let team = caller
                        .data()
                        .team_ctx
                        .as_ref()
                        .ok_or_else(|| anyhow!("tasks.spawn requires team context"))?;
                    let queue = team
                        .task_queue
                        .as_ref()
                        .ok_or_else(|| anyhow!("No task queue available"))?;

                    let spec: serde_json::Value = serde_json::from_str(&spec_json)
                        .map_err(|e| anyhow!("tasks.spawn expects JSON: {}", e))?;

                    let title = spec["title"]
                        .as_str()
                        .unwrap_or("untitled")
                        .to_string();
                    let description = spec["description"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    let id = format!("dyn-{}", uuid::Uuid::new_v4());

                    let mut task = Task::new(&id, &title, &description);
                    if let Some(assignee) = spec["assignee"].as_str() {
                        task.assignee = Some(assignee.to_string());
                    }
                    if let Some(deps) = spec["depends_on"].as_array() {
                        task.depends_on = deps
                            .iter()
                            .filter_map(|d| d.as_str().map(String::from))
                            .collect();
                    }

                    let result = {
                        let mut q = queue.lock();
                        match q.add(task) {
                            Ok(()) => serde_json::json!({"ok": true, "id": id}),
                            Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
                        }
                    };

                    let json = serde_json::to_string(&result)
                        .unwrap_or_else(|_| r#"{"ok":false}"#.into());
                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, json.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        Ok(())
    }

    fn link_skill_hostcalls(linker: &mut Linker<HostState>) -> Result<()> {
        // -- get_skill_catalog() -> string --
        linker.func_wrap0_async(
            "hugind",
            "get_skill_catalog",
            |mut caller: Caller<'_, HostState>| {
                Box::new(async move {
                    log_host(&caller, "host.skill.get_catalog");
                    let catalog = caller.data().skill_catalog.clone();
                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, catalog.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        // -- activate_skill(name) -> instructions string --
        linker.func_wrap2_async(
            "hugind",
            "activate_skill",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                Box::new(async move {
                    let name = read_string(&mut caller, ptr, len)?;
                    log_host(&caller, format!("host.skill.activate name={}", name));
                    let result = match crate::core::skill::get_skill_instructions(&name) {
                        Ok(instructions) => instructions,
                        Err(e) => format!("Error: skill '{}' not found: {}", name, e),
                    };
                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, result.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        Ok(())
    }

    fn link_agentic_hostcalls(linker: &mut Linker<HostState>) -> Result<()> {
        // -- register_tool(json) --
        linker.func_wrap(
            "hugind",
            "register_tool",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| -> Result<()> {
                let json_str = read_string(&mut caller, ptr, len)?;
                let def: serde_json::Value = serde_json::from_str(&json_str)
                    .map_err(|e| anyhow!("register_tool expects JSON: {}", e))?;
                let name = def["name"]
                    .as_str()
                    .ok_or_else(|| anyhow!("register_tool: missing 'name'"))?
                    .to_string();
                let description = def["description"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let parameters = def
                    .get("parameters")
                    .cloned()
                    .unwrap_or(serde_json::json!({}));

                log_host(&caller, format!("host.agentic.register_tool name={}", name));
                let registry = caller
                    .data()
                    .tool_registry
                    .as_ref()
                    .ok_or_else(|| anyhow!("register_tool requires agentic mode"))?;
                registry.register(AgentTool {
                    name,
                    description,
                    parameters,
                });
                Ok(())
            },
        )?;

        // -- set_system_prompt(prompt) --
        linker.func_wrap(
            "hugind",
            "set_system_prompt",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| -> Result<()> {
                let prompt = read_string(&mut caller, ptr, len)?;
                log_host(
                    &caller,
                    format!("host.agentic.set_system_prompt len={}", prompt.len()),
                );
                let registry = caller
                    .data()
                    .tool_registry
                    .as_ref()
                    .ok_or_else(|| anyhow!("set_system_prompt requires agentic mode"))?;
                registry.set_system_prompt(prompt);
                Ok(())
            },
        )?;

        // -- set_max_turns(n) --
        linker.func_wrap(
            "hugind",
            "set_max_turns",
            |caller: Caller<'_, HostState>, n: i32| -> Result<()> {
                log_host(&caller, format!("host.agentic.set_max_turns n={}", n));
                let registry = caller
                    .data()
                    .tool_registry
                    .as_ref()
                    .ok_or_else(|| anyhow!("set_max_turns requires agentic mode"))?;
                registry.set_max_turns(n as u32);
                Ok(())
            },
        )?;

        Ok(())
    }

    fn link_fs_hostcalls(linker: &mut Linker<HostState>) -> Result<()> {
        linker.func_wrap0_async(
            "hugind_fs",
            "fs_cwd",
            |mut caller: Caller<'_, HostState>| {
                Box::new(async move {
                    ensure_host_fs_enabled(&caller)?;
                    let cwd = caller.data().fs_access.cwd();
                    let cwd_str = cwd.to_string_lossy();
                    log_host(&caller, "host.fs.cwd");
                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, cwd_str.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_exists",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, ptr, len)?;
                log_host(&caller, format!("host.fs.exists path={}", path));
                Ok(if caller.data().fs_access.exists(&path)? {
                    1
                } else {
                    0
                })
            },
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_is_file",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, ptr, len)?;
                log_host(&caller, format!("host.fs.is_file path={}", path));
                Ok(if caller.data().fs_access.is_file(&path)? {
                    1
                } else {
                    0
                })
            },
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_is_dir",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, ptr, len)?;
                log_host(&caller, format!("host.fs.is_dir path={}", path));
                Ok(if caller.data().fs_access.is_dir(&path)? {
                    1
                } else {
                    0
                })
            },
        )?;

        linker.func_wrap2_async(
            "hugind_fs",
            "fs_realpath",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                Box::new(async move {
                    ensure_host_fs_enabled(&caller)?;
                    let path = read_string(&mut caller, ptr, len)?;
                    log_host(&caller, format!("host.fs.realpath path={}", path));
                    let real = caller.data().fs_access.realpath(&path)?;
                    let real_str = real.to_string_lossy();
                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, real_str.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        linker.func_wrap2_async(
            "hugind_fs",
            "fs_read_text",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                Box::new(async move {
                    ensure_host_fs_enabled(&caller)?;
                    let path = read_string(&mut caller, ptr, len)?;
                    log_host(&caller, format!("host.fs.read_text path={}", path));
                    let content = caller.data().fs_access.read_text(&path)?;
                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, content.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        linker.func_wrap2_async(
            "hugind_fs",
            "fs_read_bytes",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                Box::new(async move {
                    ensure_host_fs_enabled(&caller)?;
                    let path = read_string(&mut caller, ptr, len)?;
                    log_host(&caller, format!("host.fs.read_bytes path={}", path));
                    let content = caller.data().fs_access.read_bytes(&path)?;
                    let (out_ptr, out_len) = write_bytes_async(&mut caller, &content).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_write_text",
            |mut caller: Caller<'_, HostState>,
             path_ptr: i32,
             path_len: i32,
             data_ptr: i32,
             data_len: i32|
             -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, path_ptr, path_len)?;
                let data = read_string(&mut caller, data_ptr, data_len)?;
                log_host(
                    &caller,
                    format!("host.fs.write_text path={} bytes={}", path, data.len()),
                );
                caller.data().fs_access.write_text(&path, &data, false)?;
                Ok(0)
            },
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_write_bytes",
            |mut caller: Caller<'_, HostState>,
             path_ptr: i32,
             path_len: i32,
             data_ptr: i32,
             data_len: i32|
             -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, path_ptr, path_len)?;
                let data = read_bytes(&mut caller, data_ptr as u32, data_len as u32)?.to_vec();
                log_host(
                    &caller,
                    format!("host.fs.write_bytes path={} bytes={}", path, data.len()),
                );
                caller.data().fs_access.write_bytes(&path, &data, false)?;
                Ok(0)
            },
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_append_text",
            |mut caller: Caller<'_, HostState>,
             path_ptr: i32,
             path_len: i32,
             data_ptr: i32,
             data_len: i32|
             -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, path_ptr, path_len)?;
                let data = read_string(&mut caller, data_ptr, data_len)?;
                log_host(
                    &caller,
                    format!("host.fs.append_text path={} bytes={}", path, data.len()),
                );
                caller.data().fs_access.write_text(&path, &data, true)?;
                Ok(0)
            },
        )?;

        linker.func_wrap2_async(
            "hugind_fs",
            "fs_list_dir",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                Box::new(async move {
                    ensure_host_fs_enabled(&caller)?;
                    let path = read_string(&mut caller, ptr, len)?;
                    log_host(&caller, format!("host.fs.list_dir path={}", path));
                    let entries = caller.data().fs_access.list_dir(&path)?;
                    let json = serde_json::to_string(&entries)
                        .map_err(|e| anyhow!("failed to serialize dir entries: {}", e))?;
                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, json.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_mkdir",
            |mut caller: Caller<'_, HostState>,
             ptr: i32,
             len: i32,
             recursive: i32|
             -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, ptr, len)?;
                log_host(
                    &caller,
                    format!("host.fs.mkdir path={} recursive={}", path, recursive != 0),
                );
                caller.data().fs_access.mkdir(&path, recursive != 0)?;
                Ok(0)
            },
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_remove",
            |mut caller: Caller<'_, HostState>,
             ptr: i32,
             len: i32,
             recursive: i32|
             -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, ptr, len)?;
                log_host(
                    &caller,
                    format!("host.fs.remove path={} recursive={}", path, recursive != 0),
                );
                caller.data().fs_access.remove(&path, recursive != 0)?;
                Ok(0)
            },
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_rename",
            |mut caller: Caller<'_, HostState>,
             src_ptr: i32,
             src_len: i32,
             dst_ptr: i32,
             dst_len: i32|
             -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let src = read_string(&mut caller, src_ptr, src_len)?;
                let dst = read_string(&mut caller, dst_ptr, dst_len)?;
                log_host(&caller, format!("host.fs.rename src={} dst={}", src, dst));
                caller.data().fs_access.rename(&src, &dst)?;
                Ok(0)
            },
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_copy",
            |mut caller: Caller<'_, HostState>,
             src_ptr: i32,
             src_len: i32,
             dst_ptr: i32,
             dst_len: i32|
             -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let src = read_string(&mut caller, src_ptr, src_len)?;
                let dst = read_string(&mut caller, dst_ptr, dst_len)?;
                log_host(&caller, format!("host.fs.copy src={} dst={}", src, dst));
                caller.data().fs_access.copy(&src, &dst)?;
                Ok(0)
            },
        )?;

        linker.func_wrap2_async(
            "hugind_fs",
            "fs_stat",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                Box::new(async move {
                    ensure_host_fs_enabled(&caller)?;
                    let path = read_string(&mut caller, ptr, len)?;
                    log_host(&caller, format!("host.fs.stat path={}", path));
                    let stat = caller.data().fs_access.stat(&path)?;
                    let json = serde_json::to_string(&stat)
                        .map_err(|e| anyhow!("failed to serialize stat: {}", e))?;
                    let (out_ptr, out_len) =
                        write_bytes_async(&mut caller, json.as_bytes()).await?;
                    Ok(pack_ptr_len(out_ptr, out_len))
                })
            },
        )?;

        Ok(())
    }
}

fn extract_llm_content(body_text: &str) -> String {
    if let Ok(data) = serde_json::from_str::<serde_json::Value>(body_text) {
        if let Some(content) = data
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(|content| content.as_str())
        {
            return content.to_string();
        }
    }

    let extract_markdown_json = |text: &str| -> Option<String> {
        let start_marker = "```json";
        let end_marker = "```";
        if let Some(start_idx) = text.find(start_marker) {
            let content_start = start_idx + start_marker.len();
            if let Some(end_idx) = text[content_start..].find(end_marker) {
                return Some(text[content_start..content_start + end_idx].to_string());
            }
        }
        None
    };

    if let Some(json_str) = extract_markdown_json(body_text) {
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&json_str) {
            if let Some(content) = data
                .get("choices")
                .and_then(|choices| choices.get(0))
                .and_then(|choice| choice.get("message"))
                .and_then(|message| message.get("content"))
                .and_then(|content| content.as_str())
            {
                return content.to_string();
            }
        }
    }

    if body_text.starts_with("data: ") {
        let mut full_content = String::new();
        for line in body_text.lines() {
            if line.starts_with("data: ") {
                let data_str = &line[6..];
                if data_str.trim() == "[DONE]" {
                    continue;
                }
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(data_str) {
                    if let Some(delta) = data
                        .get("choices")
                        .and_then(|choices| choices.get(0))
                        .and_then(|choice| choice.get("delta"))
                        .and_then(|delta| delta.get("content"))
                        .and_then(|content| content.as_str())
                    {
                        full_content.push_str(delta);
                    }
                }
            }
        }
        if !full_content.is_empty() {
            return full_content;
        }
    }

    body_text.to_string()
}

fn build_wasm_llm_body(
    input: &str,
    default_model: Option<&String>,
    default_stream: bool,
    api_name: &str,
) -> Result<(serde_json::Map<String, serde_json::Value>, bool)> {
    let mut used_object_input = false;
    let trimmed = input.trim();
    let mut body = match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(serde_json::Value::Object(map)) if is_likely_llm_request_object(&map) => {
            used_object_input = true;
            map
        }
        Ok(_) | Err(_) => {
            let messages = vec![serde_json::json!({
                "role": "user",
                "content": input
            })];
            let mut map = serde_json::Map::new();
            map.insert("messages".to_string(), serde_json::json!(messages));
            map
        }
    };

    if !body.contains_key("messages") {
        if let Some(prompt_val) = body.remove("prompt") {
            if let Some(prompt) = prompt_val.as_str() {
                let messages = vec![serde_json::json!({
                    "role": "user",
                    "content": prompt
                })];
                body.insert("messages".to_string(), serde_json::json!(messages));
            } else {
                bail!("{}() prompt must be a string.", api_name);
            }
        } else {
            bail!(
                "{}() request body must include messages or prompt.",
                api_name
            );
        }
    }

    if !body.contains_key("model") {
        if let Some(model) = default_model {
            body.insert("model".to_string(), serde_json::json!(model));
        }
    }
    if !body.contains_key("stream") {
        body.insert("stream".to_string(), serde_json::json!(default_stream));
    }
    // Backward compatibility: plain-string llm_chat* calls keep JSON default,
    // but explicit object request bodies control response_format themselves.
    if !body.contains_key("response_format") && !used_object_input {
        body.insert(
            "response_format".to_string(),
            serde_json::json!({"type": "json_object"}),
        );
    }

    Ok((body, used_object_input))
}

fn is_likely_llm_request_object(map: &serde_json::Map<String, serde_json::Value>) -> bool {
    let known_keys = [
        "messages",
        "prompt",
        "model",
        "stream",
        "max_tokens",
        "temperature",
        "top_p",
        "frequency_penalty",
        "presence_penalty",
        "stop",
        "response_format",
        "enable_thinking",
        "thinking",
        "thinking_budget_tokens",
        "thinking_budget",
    ];
    known_keys.iter().any(|k| map.contains_key(*k))
}

#[derive(serde::Deserialize)]
struct ToolsCallInput {
    name: String,
    #[serde(default)]
    args: serde_json::Value,
}

fn ensure_host_fs_enabled(caller: &Caller<'_, HostState>) -> Result<()> {
    ensure_host_fs_mode_enabled(&caller.data().fs_mode)
}

fn ensure_shell_command_allowed(perm: &ShellPermission) -> Result<()> {
    crate::core::runtime::util::validate_shell_allowed(perm)
        .map_err(|e| anyhow!(e))
}

fn ensure_spawn_program_allowed(program: &str, perm: &ShellPermission) -> Result<()> {
    crate::core::runtime::util::validate_program_allowed(program, perm)
        .map_err(|e| anyhow!(e))
}

fn truncate_at_char_boundary(mut s: String, max_len: usize) -> String {
    if s.len() <= max_len {
        return s;
    }

    let mut actual_len = max_len;
    while !s.is_char_boundary(actual_len) {
        actual_len -= 1;
    }
    s.truncate(actual_len);
    s.push_str("...[truncated]");
    s
}

fn format_run_command_output(
    success: bool,
    stdout: &[u8],
    stderr: &[u8],
    max_len: usize,
) -> String {
    if success {
        let out = String::from_utf8_lossy(stdout).to_string();
        truncate_at_char_boundary(out, max_len)
    } else {
        let out = format!("Error: {}", String::from_utf8_lossy(stderr));
        truncate_at_char_boundary(out, max_len)
    }
}

fn format_spawn_output(success: bool, stdout: &[u8], stderr: &[u8], max_len: usize) -> String {
    let mut out = String::new();
    if !success {
        out.push_str("Error:\n");
    }

    let stdout_str = String::from_utf8_lossy(stdout);
    if !stdout_str.is_empty() {
        out.push_str(&stdout_str);
        if !stderr.is_empty() {
            out.push('\n');
        }
    }

    let stderr_str = String::from_utf8_lossy(stderr);
    if !stderr_str.is_empty() {
        out.push_str(&stderr_str);
    }

    truncate_at_char_boundary(out, max_len)
}

fn validate_net_fetch_target(url: &Url, permission: &NetPermissions) -> Result<(String, u16)> {
    if !permission.allow {
        bail!("Network access is disabled for this agent.");
    }

    crate::core::runtime::util::validate_http_scheme(url.scheme())
        .map_err(|e| anyhow!(e))?;

    let host = url.host_str().unwrap_or("").to_string();
    let port = url.port_or_known_default().unwrap_or(80);

    crate::core::runtime::util::validate_host_allowed(&host, permission)
        .map_err(|e| anyhow!(e))?;

    Ok((host, port))
}

fn ensure_public_network_access(
    permission: &NetPermissions,
    ips: &[std::net::IpAddr],
) -> Result<()> {
    crate::core::runtime::util::validate_public_network(permission, ips)
        .map_err(|e| anyhow!(e))
}

fn ensure_host_fs_mode_enabled(fs_mode: &RuntimeFsMode) -> Result<()> {
    match fs_mode {
        RuntimeFsMode::WasiMounts => {
            bail!("host filesystem access is disabled (runtime_fs_mode = wasi_mounts)")
        }
        RuntimeFsMode::HostFilesystem | RuntimeFsMode::Both => Ok(()),
    }
}

fn log_host(caller: &Caller<'_, HostState>, msg: impl AsRef<str>) {
    if let Some(logger) = &caller.data().logger {
        logger.log_line(msg.as_ref());
    }
}

fn read_string(caller: &mut Caller<'_, HostState>, ptr: i32, len: i32) -> Result<String> {
    if ptr < 0 || len < 0 {
        bail!("Invalid pointer/length");
    }
    let bytes = read_bytes(caller, ptr as u32, len as u32)?;
    let s = std::str::from_utf8(bytes).map_err(|e| anyhow!("Invalid UTF-8: {}", e))?;
    Ok(s.to_string())
}

fn read_bytes<'a>(caller: &'a mut Caller<'_, HostState>, ptr: u32, len: u32) -> Result<&'a [u8]> {
    let memory = get_memory(caller)?;
    let data = memory.data(caller);
    let start = ptr as usize;
    let end = start
        .checked_add(len as usize)
        .ok_or_else(|| anyhow!("Pointer overflow"))?;
    if end > data.len() {
        bail!("Memory access out of bounds");
    }
    Ok(&data[start..end])
}

async fn write_bytes_async(caller: &mut Caller<'_, HostState>, bytes: &[u8]) -> Result<(u32, u32)> {
    let alloc = caller
        .get_export("alloc")
        .and_then(|e| e.into_func())
        .ok_or_else(|| {
            anyhow!("Wasm module must export 'alloc(size: i32) -> i32' to receive data.")
        })?;

    let alloc = alloc
        .typed::<i32, i32>(&mut *caller)
        .map_err(|e| anyhow!("Invalid alloc signature: {}", e))?;

    let size = i32::try_from(
        bytes
            .len()
            .checked_add(1)
            .ok_or_else(|| anyhow!("Allocation size overflow"))?,
    )
    .map_err(|_| anyhow!("Allocation size exceeds i32::MAX"))?;
    let start = alloc
        .call_async(&mut *caller, size)
        .await
        .map_err(|e| anyhow!("alloc failed: {}", e))?;

    if start < 0 {
        bail!("alloc returned negative pointer");
    }

    let start = start as usize;
    let end = start
        .checked_add(bytes.len())
        .ok_or_else(|| anyhow!("Pointer overflow"))?;
    let end_with_nul = end
        .checked_add(1)
        .ok_or_else(|| anyhow!("Pointer overflow"))?;

    let memory = get_memory(caller)?;
    let data = memory.data_mut(&mut *caller);
    if end_with_nul > data.len() {
        bail!("Guest allocation out of bounds");
    }
    data[start..end].copy_from_slice(bytes);
    data[end] = 0;
    Ok((start as u32, bytes.len() as u32))
}

fn get_memory(caller: &mut Caller<'_, HostState>) -> Result<wasmtime::Memory> {
    let memory = caller
        .get_export("memory")
        .and_then(|e| e.into_memory())
        .ok_or_else(|| anyhow!("missing exported memory"))?;
    Ok(memory)
}

fn pack_ptr_len(ptr: u32, len: u32) -> i64 {
    ((ptr as i64) << 32) | (len as i64 & 0xffff_ffff)
}

#[cfg(test)]
mod tests {
    use super::{
        build_wasm_llm_body, ensure_host_fs_mode_enabled, ensure_public_network_access,
        ensure_shell_command_allowed, ensure_spawn_program_allowed, extract_llm_content,
        format_run_command_output, format_spawn_output, is_likely_llm_request_object, pack_ptr_len,
        validate_net_fetch_target,
    };
    use crate::core::config::agent::{NetPermissions, RuntimeFsMode, ShellPermission};
    use reqwest::Url;
    use serde_json::json;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn build_wasm_llm_body_from_plain_prompt_sets_defaults() {
        let default_model = Some("test-model".to_string());
        let (body, used_object_input) =
            build_wasm_llm_body("hello", default_model.as_ref(), false, "llm_chat").expect("body");

        assert!(!used_object_input);
        assert_eq!(body.get("model"), Some(&json!("test-model")));
        assert_eq!(body.get("stream"), Some(&json!(false)));
        assert_eq!(
            body.get("response_format"),
            Some(&json!({"type": "json_object"}))
        );
        assert_eq!(
            body.get("messages"),
            Some(&json!([{"role":"user","content":"hello"}]))
        );
    }

    #[test]
    fn build_wasm_llm_body_converts_prompt_in_object_request() {
        let (body, used_object_input) = build_wasm_llm_body(
            r#"{"prompt":"hello from prompt"}"#,
            None,
            true,
            "llm_chat_stream",
        )
        .expect("body");

        assert!(used_object_input);
        assert_eq!(body.get("stream"), Some(&json!(true)));
        assert_eq!(
            body.get("messages"),
            Some(&json!([{"role":"user","content":"hello from prompt"}]))
        );
        assert!(!body.contains_key("prompt"));
        assert!(!body.contains_key("response_format"));
    }

    #[test]
    fn build_wasm_llm_body_errors_when_prompt_is_not_string() {
        let err = build_wasm_llm_body(r#"{"prompt":123}"#, None, false, "llm_chat")
            .expect_err("must fail");
        assert!(
            err.to_string()
                .contains("llm_chat() prompt must be a string.")
        );
    }

    #[test]
    fn build_wasm_llm_body_errors_when_messages_and_prompt_missing() {
        let err = build_wasm_llm_body(r#"{"temperature":0.2}"#, None, false, "llm_chat")
            .expect_err("must fail");
        assert!(
            err.to_string()
                .contains("llm_chat() request body must include messages or prompt.")
        );
    }

    #[test]
    fn build_wasm_llm_body_preserves_object_response_format() {
        let (body, used_object_input) = build_wasm_llm_body(
            r#"{"messages":[{"role":"user","content":"hi"}],"response_format":{"type":"text"}}"#,
            None,
            false,
            "llm_chat",
        )
        .expect("body");

        assert!(used_object_input);
        assert_eq!(body.get("stream"), Some(&json!(false)));
        assert_eq!(body.get("response_format"), Some(&json!({"type":"text"})));
    }

    #[test]
    fn likely_llm_request_object_detects_known_keys() {
        let with_messages = serde_json::from_value(json!({"messages": []})).expect("object");
        let with_thinking_budget =
            serde_json::from_value(json!({"thinking_budget": 128})).expect("object");
        let unknown = serde_json::from_value(json!({"foo": "bar"})).expect("object");

        assert!(is_likely_llm_request_object(&with_messages));
        assert!(is_likely_llm_request_object(&with_thinking_budget));
        assert!(!is_likely_llm_request_object(&unknown));
    }

    #[test]
    fn extract_llm_content_from_standard_json_payload() {
        let text = r#"{"choices":[{"message":{"content":"hello"}}]}"#;
        assert_eq!(extract_llm_content(text), "hello");
    }

    #[test]
    fn extract_llm_content_from_markdown_wrapped_json() {
        let text =
            "prefix ```json{\"choices\":[{\"message\":{\"content\":\"wrapped\"}}]}``` suffix";
        assert_eq!(extract_llm_content(text), "wrapped");
    }

    #[test]
    fn extract_llm_content_from_sse_stream() {
        let text = "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\
data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\
data: [DONE]\n";
        assert_eq!(extract_llm_content(text), "hello");
    }

    #[test]
    fn extract_llm_content_falls_back_to_original_text() {
        let raw = "not-json content";
        assert_eq!(extract_llm_content(raw), raw);
    }

    #[test]
    fn pack_ptr_len_combines_pointer_and_length() {
        let packed = pack_ptr_len(0x1234_5678, 0x9abc_def0);
        let ptr = ((packed >> 32) & 0xffff_ffff) as u32;
        let len = (packed & 0xffff_ffff) as u32;
        assert_eq!(ptr, 0x1234_5678);
        assert_eq!(len, 0x9abc_def0);
    }

    #[test]
    fn run_command_permission_rejects_when_disabled() {
        let perm = ShellPermission::default();
        let err = ensure_shell_command_allowed(&perm).expect_err("must reject");
        assert!(err.to_string().contains("Shell execution is disabled."));
    }

    #[test]
    fn run_command_permission_allows_when_enabled() {
        let mut perm = ShellPermission::default();
        perm.allow = true;
        assert!(ensure_shell_command_allowed(&perm).is_ok());
    }

    #[test]
    fn spawn_permission_enforces_whitelist_and_blacklist() {
        let mut perm = ShellPermission::default();
        perm.allow = true;
        perm.whitelist = Some(vec!["echo".to_string()]);
        perm.blacklist = Some(vec!["rm".to_string()]);

        assert!(ensure_spawn_program_allowed("echo", &perm).is_ok());
        assert!(ensure_spawn_program_allowed("ls", &perm).is_err());
        assert!(ensure_spawn_program_allowed("rm", &perm).is_err());
    }

    #[test]
    fn run_command_output_formats_and_truncates() {
        let ok = format_run_command_output(true, b"abcdef", b"", 3);
        assert_eq!(ok, "abc...[truncated]");

        let err = format_run_command_output(false, b"", b"oops", 64);
        assert_eq!(err, "Error: oops");
    }

    #[test]
    fn spawn_output_formats_and_truncates() {
        let err = format_spawn_output(false, b"out", b"err", 64);
        assert_eq!(err, "Error:\nout\nerr");

        let truncated = format_spawn_output(true, b"abcdef", b"", 4);
        assert_eq!(truncated, "abcd...[truncated]");
    }

    #[test]
    fn net_target_rejects_when_network_is_disabled() {
        let perm = NetPermissions::default();
        let url = Url::parse("https://example.com").expect("url");
        let err = validate_net_fetch_target(&url, &perm).expect_err("must reject");
        assert!(
            err.to_string()
                .contains("Network access is disabled for this agent.")
        );
    }

    #[test]
    fn net_target_rejects_non_http_scheme() {
        let mut perm = NetPermissions::default();
        perm.allow = true;
        let url = Url::parse("ftp://example.com").expect("url");
        let err = validate_net_fetch_target(&url, &perm).expect_err("must reject");
        assert!(err.to_string().contains("URL scheme 'ftp' is not allowed."));
    }

    #[test]
    fn net_target_honors_domain_and_ip_allowlists() {
        let mut perm = NetPermissions::default();
        perm.allow = true;
        perm.allowed_domains = vec!["example.com".to_string()];
        perm.allowed_ips = vec!["127.0.0.1".to_string()];

        let domain = Url::parse("https://api.example.com/path").expect("url");
        let ip = Url::parse("http://127.0.0.1:8080").expect("url");
        let blocked = Url::parse("https://blocked.com").expect("url");

        assert!(validate_net_fetch_target(&domain, &perm).is_ok());
        assert!(validate_net_fetch_target(&ip, &perm).is_ok());
        assert!(validate_net_fetch_target(&blocked, &perm).is_err());
    }

    #[test]
    fn private_network_access_blocks_private_ips_when_enabled() {
        let mut perm = NetPermissions::default();
        perm.block_private_networks = true;
        let ips = vec![IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))];
        let err = ensure_public_network_access(&perm, &ips).expect_err("must reject");
        assert!(
            err.to_string()
                .contains("Access to private network blocked (IP: 127.0.0.1)")
        );
    }

    #[test]
    fn private_network_access_allows_private_ips_when_disabled() {
        let perm = NetPermissions::default();
        let ips = vec![IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))];
        assert!(ensure_public_network_access(&perm, &ips).is_ok());
    }

    #[test]
    fn host_fs_mode_gating_matches_runtime_mode() {
        assert!(ensure_host_fs_mode_enabled(&RuntimeFsMode::HostFilesystem).is_ok());
        assert!(ensure_host_fs_mode_enabled(&RuntimeFsMode::Both).is_ok());
        let err = ensure_host_fs_mode_enabled(&RuntimeFsMode::WasiMounts).expect_err("must fail");
        assert!(
            err.to_string()
                .contains("host filesystem access is disabled (runtime_fs_mode = wasi_mounts)")
        );
    }
}
