use anyhow::{anyhow, bail, Context, Result};
use reqwest::Url;
use std::path::{Path, PathBuf};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use wasmtime::{Caller, Config, Engine, Linker, Module, Store};
use wasmtime::component::ResourceTable;
use wasmtime_wasi::preview2::{WasiCtx, WasiCtxBuilder, WasiView, DirPerms, FilePerms};
use wasmtime_wasi::preview2::preview1::{WasiPreview1View, WasiPreview1Adapter};
use cap_std::fs::Dir as CapDir;
use futures::StreamExt; 
use crate::core::config::agent::{AgentConfig, NetPermissions, ShellPermission, RuntimeFsMode};
use crate::core::fs::FsAccess;
use crate::core::config::backend::resolve_backend;

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
    wasi: WasiCtx,
    table: ResourceTable,
    adapter: WasiPreview1Adapter,
    limits: wasmtime::StoreLimits,
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
    config: AgentConfig,
}

impl WasmRuntime {
    pub fn new(agent_root: PathBuf, config: &AgentConfig) -> Result<Self> {
        let mut wasm_config = Config::new();
        wasm_config.async_support(true);

        if let Some(wasm_opts) = &config.wasm {
            if let Some(resources) = &wasm_opts.resources {
                
                if resources.cpu.is_some() {
                    wasm_config.consume_fuel(true);
                }
                
                
                if let Some(mem_str) = &resources.memory {
                   if let Some(_bytes) = parse_memory_string(mem_str) {
                       
                   }
                }
            }
        }

        let engine = Engine::new(&wasm_config)?;
        Ok(Self {
            engine,
            agent_root,
            config: config.clone(),
        })
    }

    pub async fn run_module(&self, entry: &Path, args_val: serde_json::Value) -> Result<serde_json::Value> {
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
            self.agent_root.clone(),
            self.config.permissions.as_ref().and_then(|p| p.filesystem.clone()),
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
                        let host_path = Path::new(&mount.host).canonicalize()
                            .with_context(|| format!("failed to canonicalize host path: {}", mount.host))?;

                        let guest_path = Path::new(&mount.guest);
                        if !guest_path.is_absolute() {
                            bail!("mount guest path must be absolute: {}", mount.guest);
                        }
                        if guest_path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                            bail!("mount guest path must not contain '..': {}", mount.guest);
                        }

                        if !allow_outside_agent_root && !host_path.starts_with(&self.agent_root) {
                            bail!(
                                "mount '{}' is outside agent root; set permissions.filesystem.allow_outside_agent_root=true to allow",
                                host_path.display()
                            );
                        }
                            
                        let dir = CapDir::open_ambient_dir(&host_path, cap_std::ambient_authority())
                            .with_context(|| format!("failed to open host dir: {:?}", host_path))?;
                            
                        wasi_builder.preopened_dir(dir, DirPerms::all(), FilePerms::all(), &mount.guest);
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
                wasi,
                table,
                adapter,
                limits,
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
                 if let Some(_cpu) = &res.cpu {
                     
                     
                     store.set_fuel(1_000_000_000).ok(); 
                 }
             }
        }

        let module = Module::from_file(&self.engine, &entry)
            .with_context(|| format!("failed to load wasm module: {}", entry.display()))?;

        let mut linker = Linker::new(&self.engine);
        
        
        wasmtime_wasi::preview2::preview1::add_to_linker_async(&mut linker)?;

        Self::link_host_functions(&mut linker)?;

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

    fn link_host_functions(linker: &mut Linker<HostState>) -> Result<()> {
        linker.func_wrap("env", "abort", |mut _caller: Caller<'_, HostState>, _msg: i32, _file: i32, line: i32, col: i32| -> Result<()> {
            eprintln!("Guest Error: abort() called at line {}:{}", line, col);
            Err(anyhow::anyhow!("Guest execution aborted"))
        })?;

        linker.func_wrap("hugind", "print", |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
            let msg = read_string(&mut caller, ptr, len)?;
            println!("{msg}");
            Ok(())
        })?;

        linker.func_wrap2_async(
            "hugind",
            "input",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| Box::new(async move {
                let prompt = read_string(&mut caller, ptr, len)?;
                let mut stdout = io::stdout();
                let _ = stdout.write_all(prompt.as_bytes()).await;
                let _ = stdout.flush().await;

                let mut reader = BufReader::new(io::stdin());
                let mut buffer = String::new();
                let _ = reader.read_line(&mut buffer).await;
                let value = buffer.trim().to_string();
                let (out_ptr, out_len) = write_bytes_async(&mut caller, value.as_bytes()).await?;
                Ok(pack_ptr_len(out_ptr, out_len))
            }),
        )?;

        
        linker.func_wrap2_async(
            "hugind",
            "run_command",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| Box::new(async move {
                let cmd_str = read_string(&mut caller, ptr, len)?;
                
                
                let perm = caller.data().shell_permission.clone().unwrap_or_default();
                if !perm.allow {
                     bail!("Shell execution is disabled.");
                }
                
                let parts: Vec<&str> = cmd_str.split_whitespace().collect();
                if parts.is_empty() {
                    bail!("Empty command");
                }
                let program = parts[0];

                
                if let Some(whitelist) = &perm.whitelist {
                    if !whitelist.iter().any(|cmd| cmd == program) {
                        bail!("Command '{}' is not whitelisted.", program);
                    }
                }

                
                if let Some(blacklist) = &perm.blacklist {
                    if blacklist.iter().any(|cmd| cmd == program) {
                        bail!("Command '{}' is blacklisted.", program);
                    }
                }

                
                let mut command = if cfg!(target_os = "macos") {
                    
                    
                    let profile = "(version 1) (allow default)";
                    let mut cmd = Command::new("sandbox-exec");
                    cmd.arg("-p").arg(profile).arg(program);
                    cmd
                } else {
                    Command::new(program)
                };
                
                if parts.len() > 1 {
                    command.args(&parts[1..]);
                }

                
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

                let output = output_res.map_err(|e| anyhow!("Failed to execute command: {}", e))?;

                
                let max_len = perm.max_output.as_deref().and_then(parse_memory_string).unwrap_or(1024 * 1024); 

                let result_str = if output.status.success() {
                    if output.stdout.len() > max_len {
                        let mut s = String::from_utf8_lossy(&output.stdout[..max_len]).to_string();
                        s.push_str("...[truncated]");
                        s
                    } else {
                        String::from_utf8_lossy(&output.stdout).to_string()
                    }
                } else {
                     let mut s = format!("Error: {}", String::from_utf8_lossy(&output.stderr));
                     if s.len() > max_len {
                        let mut actual_len = max_len;
                        while !s.is_char_boundary(actual_len) {
                            actual_len -= 1;
                        }
                        s.truncate(actual_len);
                        s.push_str("...[truncated]");
                     }
                     s
                };
                
                let (out_ptr, out_len) = write_bytes_async(&mut caller, result_str.as_bytes()).await?;
                Ok(pack_ptr_len(out_ptr, out_len))
            }),
        )?;

        linker.func_wrap2_async(
            "hugind",
            "net_fetch",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| Box::new(async move {
                let url = read_string(&mut caller, ptr, len)?;
                let permission = caller.data().net_permission.clone();
                let client = caller.data().net_client.clone();
                if !permission.allow {
                    bail!("Network access is disabled for this agent.");
                }

                let mut current = Url::parse(&url).map_err(|e| anyhow!("Invalid URL: {}", e))?;
                let max_redirects = 5;

                for _ in 0..=max_redirects {
                    match current.scheme() {
                        "http" | "https" => {}
                        _ => bail!("URL scheme '{}' is not allowed.", current.scheme()),
                    }

                    let host = current.host_str().unwrap_or("");
                    let port = current.port_or_known_default().unwrap_or(80);

                    
                    if !permission.allowed_domains.is_empty() || !permission.allowed_ips.is_empty() {
                        let allowed = permission
                            .allowed_domains
                            .iter()
                            .any(|d| host == d || host.ends_with(&format!(".{}", d)));

                        if !allowed {
                            
                            let is_ip_allowed = if let Ok(_ip) = host.parse::<std::net::IpAddr>() {
                                permission.allowed_ips.iter().any(|allowed_ip| allowed_ip == host)
                            } else {
                                false
                            };
                            if !is_ip_allowed {
                                bail!("Domain/IP '{}' is not in the allowed list.", host);
                            }
                        }
                    }

                    
                    
                    
                    if permission.block_private_networks {
                        
                        let ips: Vec<std::net::IpAddr> =
                            if let Ok(ip) = host.parse::<std::net::IpAddr>() {
                                vec![ip]
                            } else {
                                
                                let addr_str = format!("{}:{}", host, port);
                                tokio::net::lookup_host(&addr_str)
                                    .await
                                    .map_err(|e| {
                                        anyhow!("DNS resolution failed for {}: {}", host, e)
                                    })?
                                    .map(|sa| sa.ip())
                                    .collect()
                            };

                        for ip in ips {
                            if is_private_ip(&ip) {
                                bail!("Access to private network blocked (IP: {})", ip);
                            }
                        }
                    }

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

                    let (out_ptr, out_len) = write_bytes_async(&mut caller, text.as_bytes()).await?;
                    return Ok(pack_ptr_len(out_ptr, out_len));
                }

                bail!("Too many redirects");
            }),
        )?;

        linker.func_wrap2_async(
            "hugind",
            "llm_chat",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| Box::new(async move {
                let prompt = read_string(&mut caller, ptr, len)?;
                let base_url = caller.data().llm_base_url.clone();
                let model = caller.data().llm_model.clone();
                let client = caller.data().llm_client.clone();

                let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
                let messages = vec![serde_json::json!({
                    "role": "user",
                    "content": prompt
                })];

                let mut body = serde_json::Map::new();
                if let Some(m) = &model {
                    body.insert("model".to_string(), serde_json::json!(m));
                }
                body.insert("messages".to_string(), serde_json::json!(messages));
                body.insert("stream".to_string(), serde_json::json!(false));
                body.insert(
                    "response_format".to_string(),
                    serde_json::json!({"type": "json_object"})
                );

                
                let res = client
                    .post(&url)
                    .timeout(std::time::Duration::from_secs(120)) 
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| anyhow!("LLM Request Failed: {}", e))?;

                if !res.status().is_success() {
                    let status = res.status();
                    
                     let bytes = res.bytes().await.unwrap_or_default();
                     let text = String::from_utf8_lossy(&bytes);
                    bail!("LLM Error: {}: {}", status, text);
                }

                
                let max_bytes = 10 * 1024 * 1024; 
                let mut content = Vec::new();
                let mut stream = res.bytes_stream();
                
                 while let Some(item) = stream.next().await {
                    let chunk = item.map_err(|e| anyhow!("LLM Chunk error: {}", e))?;
                    if content.len() + chunk.len() > max_bytes {
                         bail!("LLM Response exceeded 10MB limit");
                    }
                    content.extend_from_slice(&chunk);
                }

                let body_text = String::from_utf8_lossy(&content).to_string();

                let content = extract_llm_content(&body_text);
                let (out_ptr, out_len) = write_bytes_async(&mut caller, content.as_bytes()).await?;
                Ok(pack_ptr_len(out_ptr, out_len))
            }),
        )?;

        linker.func_wrap2_async(
            "hugind",
            "llm_chat_stream",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| Box::new(async move {
                let prompt = read_string(&mut caller, ptr, len)?;
                let base_url = caller.data().llm_base_url.clone();
                let model = caller.data().llm_model.clone();
                let client = caller.data().llm_client.clone();

                let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
                let messages = vec![serde_json::json!({
                    "role": "user",
                    "content": prompt
                })];

                let mut body = serde_json::Map::new();
                if let Some(m) = &model {
                    body.insert("model".to_string(), serde_json::json!(m));
                }
                body.insert("messages".to_string(), serde_json::json!(messages));
                body.insert("stream".to_string(), serde_json::json!(true));
                body.insert(
                    "response_format".to_string(),
                    serde_json::json!({"type": "json_object"})
                );

                
                let res = client
                    .post(&url)
                    .timeout(std::time::Duration::from_secs(120))
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| anyhow!("LLM Request Failed: {}", e))?;

                if !res.status().is_success() {
                    let status = res.status();
                    let bytes = res.bytes().await.unwrap_or_default();
                    let text = String::from_utf8_lossy(&bytes);
                    bail!("LLM Error: {}: {}", status, text);
                }

                
                let max_bytes = 10 * 1024 * 1024;
                let mut content = String::new();
                let mut stream = res.bytes_stream();

                while let Some(item) = stream.next().await {
                    let chunk = item.map_err(|e| anyhow!("LLM Chunk error: {}", e))?;
                    if content.len() + chunk.len() > max_bytes {
                        bail!("LLM Response exceeded 10MB limit");
                    }
                    let text = String::from_utf8_lossy(&chunk);
                    for line in text.lines() {
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
                                
                                print!("{delta}");
                            }
                        }
                    }
                }

                let (out_ptr, out_len) = write_bytes_async(&mut caller, content.as_bytes()).await?;
                Ok(pack_ptr_len(out_ptr, out_len))
            }),
        )?;

        linker.func_wrap0_async(
            "hugind",
            "get_args",
            |mut caller: Caller<'_, HostState>| Box::new(async move {
                let args_json = caller.data().args_json.clone();
                let (out_ptr, out_len) = write_bytes_async(&mut caller, args_json.as_bytes()).await?;
                Ok(pack_ptr_len(out_ptr, out_len))
            }),
        )?;

        linker.func_wrap(
            "hugind",
            "set_result",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                let json_str = read_string(&mut caller, ptr, len)?;
                let parsed = serde_json::from_str::<serde_json::Value>(&json_str)
                    .map_err(|e| anyhow!("Invalid JSON result: {}", e))?;
                caller.data_mut().result_json = Some(parsed);
                Ok(())
            },
        )?;

        Self::link_fs_hostcalls(linker)?;

        Ok(())
    }

    fn link_fs_hostcalls(linker: &mut Linker<HostState>) -> Result<()> {
        
        linker.func_wrap0_async(
            "hugind_fs",
            "fs_cwd",
            |mut caller: Caller<'_, HostState>| Box::new(async move {
                ensure_host_fs_enabled(&caller)?;
                let cwd = caller.data().fs_access.cwd();
                let cwd_str = cwd.to_string_lossy();
                let (out_ptr, out_len) = write_bytes_async(&mut caller, cwd_str.as_bytes()).await?;
                Ok(pack_ptr_len(out_ptr, out_len))
            }),
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_exists",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, ptr, len)?;
                Ok(if caller.data().fs_access.exists(&path)? { 1 } else { 0 })
            },
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_is_file",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, ptr, len)?;
                Ok(if caller.data().fs_access.is_file(&path)? { 1 } else { 0 })
            },
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_is_dir",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, ptr, len)?;
                Ok(if caller.data().fs_access.is_dir(&path)? { 1 } else { 0 })
            },
        )?;

        linker.func_wrap2_async(
            "hugind_fs",
            "fs_realpath",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| Box::new(async move {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, ptr, len)?;
                let real = caller.data().fs_access.realpath(&path)?;
                let real_str = real.to_string_lossy();
                let (out_ptr, out_len) = write_bytes_async(&mut caller, real_str.as_bytes()).await?;
                Ok(pack_ptr_len(out_ptr, out_len))
            }),
        )?;

        linker.func_wrap2_async(
            "hugind_fs",
            "fs_read_text",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| Box::new(async move {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, ptr, len)?;
                let content = caller.data().fs_access.read_text(&path)?;
                let (out_ptr, out_len) = write_bytes_async(&mut caller, content.as_bytes()).await?;
                Ok(pack_ptr_len(out_ptr, out_len))
            }),
        )?;

        linker.func_wrap2_async(
            "hugind_fs",
            "fs_read_bytes",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| Box::new(async move {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, ptr, len)?;
                let content = caller.data().fs_access.read_bytes(&path)?;
                let (out_ptr, out_len) = write_bytes_async(&mut caller, &content).await?;
                Ok(pack_ptr_len(out_ptr, out_len))
            }),
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_write_text",
            |mut caller: Caller<'_, HostState>, path_ptr: i32, path_len: i32, data_ptr: i32, data_len: i32| -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, path_ptr, path_len)?;
                let data = read_string(&mut caller, data_ptr, data_len)?;
                caller.data().fs_access.write_text(&path, &data, false)?;
                Ok(0)
            },
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_write_bytes",
            |mut caller: Caller<'_, HostState>, path_ptr: i32, path_len: i32, data_ptr: i32, data_len: i32| -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, path_ptr, path_len)?;
                let data = read_bytes(&mut caller, data_ptr as u32, data_len as u32)?.to_vec();
                caller.data().fs_access.write_bytes(&path, &data, false)?;
                Ok(0)
            },
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_append_text",
            |mut caller: Caller<'_, HostState>, path_ptr: i32, path_len: i32, data_ptr: i32, data_len: i32| -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, path_ptr, path_len)?;
                let data = read_string(&mut caller, data_ptr, data_len)?;
                caller.data().fs_access.write_text(&path, &data, true)?;
                Ok(0)
            },
        )?;

        linker.func_wrap2_async(
            "hugind_fs",
            "fs_list_dir",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| Box::new(async move {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, ptr, len)?;
                let entries = caller.data().fs_access.list_dir(&path)?;
                let json = serde_json::to_string(&entries)
                    .map_err(|e| anyhow!("failed to serialize dir entries: {}", e))?;
                let (out_ptr, out_len) = write_bytes_async(&mut caller, json.as_bytes()).await?;
                Ok(pack_ptr_len(out_ptr, out_len))
            }),
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_mkdir",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32, recursive: i32| -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, ptr, len)?;
                caller.data().fs_access.mkdir(&path, recursive != 0)?;
                Ok(0)
            },
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_remove",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32, recursive: i32| -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, ptr, len)?;
                caller.data().fs_access.remove(&path, recursive != 0)?;
                Ok(0)
            },
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_rename",
            |mut caller: Caller<'_, HostState>, src_ptr: i32, src_len: i32, dst_ptr: i32, dst_len: i32| -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let src = read_string(&mut caller, src_ptr, src_len)?;
                let dst = read_string(&mut caller, dst_ptr, dst_len)?;
                caller.data().fs_access.rename(&src, &dst)?;
                Ok(0)
            },
        )?;

        linker.func_wrap(
            "hugind_fs",
            "fs_copy",
            |mut caller: Caller<'_, HostState>, src_ptr: i32, src_len: i32, dst_ptr: i32, dst_len: i32| -> Result<i32> {
                ensure_host_fs_enabled(&caller)?;
                let src = read_string(&mut caller, src_ptr, src_len)?;
                let dst = read_string(&mut caller, dst_ptr, dst_len)?;
                caller.data().fs_access.copy(&src, &dst)?;
                Ok(0)
            },
        )?;

        linker.func_wrap2_async(
            "hugind_fs",
            "fs_stat",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| Box::new(async move {
                ensure_host_fs_enabled(&caller)?;
                let path = read_string(&mut caller, ptr, len)?;
                let stat = caller.data().fs_access.stat(&path)?;
                let json = serde_json::to_string(&stat)
                    .map_err(|e| anyhow!("failed to serialize stat: {}", e))?;
                let (out_ptr, out_len) = write_bytes_async(&mut caller, json.as_bytes()).await?;
                Ok(pack_ptr_len(out_ptr, out_len))
            }),
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

fn ensure_host_fs_enabled(caller: &Caller<'_, HostState>) -> Result<()> {
    match caller.data().fs_mode {
        RuntimeFsMode::WasiMounts => bail!("host filesystem access is disabled (runtime_fs_mode = wasi_mounts)"),
        RuntimeFsMode::HostFilesystem | RuntimeFsMode::Both => Ok(()),
    }
}

fn read_string(caller: &mut Caller<'_, HostState>, ptr: i32, len: i32) -> Result<String> {
    if ptr < 0 || len < 0 {
        bail!("Invalid pointer/length");
    }
    let bytes = read_bytes(caller, ptr as u32, len as u32)?;
    let s = std::str::from_utf8(bytes)
        .map_err(|e| anyhow!("Invalid UTF-8: {}", e))?;
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
        .ok_or_else(|| anyhow!("Wasm module must export 'alloc(size: i32) -> i32' to receive data."))?;

    let alloc = alloc
        .typed::<i32, i32>(&mut *caller)
        .map_err(|e| anyhow!("Invalid alloc signature: {}", e))?;

    let size = i32::try_from(bytes.len().checked_add(1).ok_or_else(|| anyhow!("Allocation size overflow"))?)
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

fn parse_memory_string(mem: &str) -> Option<usize> {
    
    let mem = mem.trim().to_uppercase();
    if let Some(stripped) = mem.strip_suffix("MB") {
        return stripped.parse::<usize>().ok().map(|v| v * 1024 * 1024);
    }
    if let Some(stripped) = mem.strip_suffix("GB") {
        return stripped.parse::<usize>().ok().map(|v| v * 1024 * 1024 * 1024);
    }
    mem.parse::<usize>().ok()
}

fn parse_duration_string(s: &str) -> Option<std::time::Duration> {
    let s = s.trim();
    if let Some(ms) = s.strip_suffix("ms") {
        return ms.parse::<u64>().ok().map(std::time::Duration::from_millis);
    }
    if let Some(sec) = s.strip_suffix("s") {
        return sec.parse::<u64>().ok().map(std::time::Duration::from_secs);
    }
    if let Some(min) = s.strip_suffix("m") {
        return min.parse::<u64>().ok().map(|min| std::time::Duration::from_secs(min * 60));
    }
    
    s.parse::<u64>().ok().map(std::time::Duration::from_secs)
}

fn is_private_ip(ip: &std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(addr) => {
            let octets = addr.octets();
            
            if octets[0] == 10 { return true; }
            
            if octets[0] == 172 && (octets[1] >= 16 && octets[1] <= 31) { return true; }
            
            if octets[0] == 192 && octets[1] == 168 { return true; }
            
            if octets[0] == 127 { return true; }
            
            if octets[0] == 169 && octets[1] == 254 { return true; }
            
            if octets[0] == 100 && (octets[1] >= 64 && octets[1] <= 127) { return true; }
            
            if octets[0] == 0 { return true; }
            false
        }
        std::net::IpAddr::V6(addr) => {
            
            if addr.is_loopback() { return true; }
            let segments = addr.segments();
            
            if (segments[0] & 0xfe00) == 0xfc00 { return true; }
            
            if (segments[0] & 0xffc0) == 0xfe80 { return true; }
            false
        }
    }
}
