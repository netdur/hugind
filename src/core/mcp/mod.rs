use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex as AsyncMutex, oneshot};

use crate::core::config::agent::AgentConfig;
#[derive(Debug, Deserialize, Clone)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: Option<String>,
    pub command: String,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub cwd: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct McpDependency {
    pub name: String,
    pub version: Option<String>,
    #[serde(default)]
    pub required: bool,
    pub description: Option<String>,
    pub transport: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<serde_json::Value>,
    pub server: String,
}

pub struct McpManager {
    clients: HashMap<String, McpClient>,
}

impl McpManager {
    pub async fn new(config: &AgentConfig) -> Result<Option<Self>> {
        let deps = parse_mcp_dependencies(config)?;
        if deps.is_empty() {
            return Ok(None);
        }
        let mut clients = HashMap::new();

        for dep in deps {
            let Some(server) = dependency_to_server(&dep) else {
                if dep.required {
                    bail!("Required MCP server '{}' is missing command configuration", dep.name);
                }
                continue;
            };

            let client = McpClient::spawn(server.clone())
                .await
                .with_context(|| format!("Failed to start MCP server '{}'", server.name))?;
            clients.insert(server.name.clone(), client);
        }

        Ok(Some(Self { clients }))
    }

    pub async fn list_tools(&self) -> Result<Vec<ToolInfo>> {
        let mut out = Vec::new();
        if self.clients.is_empty() {
            return Ok(out);
        }
        for (server, client) in &self.clients {
            let tools = client.list_tools().await?;
            for tool in tools {
                out.push(ToolInfo {
                    name: format!("{}:{}", server, tool.name),
                    description: tool.description,
                    input_schema: tool.input_schema,
                    server: server.clone(),
                });
            }
        }
        Ok(out)
    }

    pub async fn call_tool(&self, name: &str, args: serde_json::Value) -> Result<serde_json::Value> {
        if self.clients.is_empty() {
            bail!("No MCP tools configured");
        }
        let (server, tool) = self.resolve_tool(name)?;
        let client = self.clients.get(&server).ok_or_else(|| {
            anyhow::anyhow!("MCP server '{}' is not available", server)
        })?;
        client.call_tool(&tool, args).await
    }

    fn resolve_tool(&self, name: &str) -> Result<(String, String)> {
        if let Some((server, tool)) = name.split_once(':') {
            return Ok((server.to_string(), tool.to_string()));
        }
        if self.clients.len() == 1 {
            let server = self.clients.keys().next().unwrap().to_string();
            return Ok((server, name.to_string()));
        }
        bail!("Tool name '{}' is ambiguous; use 'server:tool'", name);
    }
}

#[derive(Debug, Clone)]
struct ToolDescriptor {
    name: String,
    description: Option<String>,
    input_schema: Option<serde_json::Value>,
}

struct McpClient {
    name: String,
    stdin: Arc<AsyncMutex<ChildStdin>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>>,
    next_id: AtomicU64,
    _child: Child,
}

impl McpClient {
    async fn spawn(config: McpServerConfig) -> Result<Self> {
        let transport = config.transport.clone().unwrap_or_else(|| "stdio".to_string());
        if transport != "stdio" {
            bail!("Unsupported MCP transport '{}'", transport);
        }

        let mut cmd = Command::new(&config.command);
        if let Some(args) = &config.args {
            cmd.args(args);
        }
        if let Some(env) = &config.env {
            cmd.envs(env);
        }
        if let Some(cwd) = &config.cwd {
            cmd.current_dir(cwd);
        }
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::inherit());

        let mut child = cmd.spawn()
            .with_context(|| format!("Failed to spawn MCP server '{}'", config.name))?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("Failed to open stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("Failed to open stdout"))?;

        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        start_stdout_reader(stdout, pending.clone(), config.name.clone());

        let client = Self {
            name: config.name.clone(),
            stdin: Arc::new(AsyncMutex::new(stdin)),
            pending,
            next_id: AtomicU64::new(1),
            _child: child,
        };

        client.initialize().await?;
        Ok(client)
    }

    async fn initialize(&self) -> Result<()> {
        let params = json!({
            "protocolVersion": "2024-11-05",
            "clientInfo": { "name": "hugind", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": {}
        });
        let _ = self.request("initialize", params).await?;
        self.notify("notifications/initialized", json!({})).await?;
        Ok(())
    }

    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let params = json!({
                "cursor": cursor,
            });
            let result = self.request("tools/list", params).await?;
            if let Some(items) = result.get("tools").and_then(|v| v.as_array()) {
                for item in items {
                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let description = item.get("description").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let input_schema = item.get("inputSchema").cloned();
                    if !name.is_empty() {
                        tools.push(ToolDescriptor { name, description, input_schema });
                    }
                }
            }
            cursor = result.get("nextCursor").and_then(|v| v.as_str()).map(|s| s.to_string());
            if cursor.is_none() {
                break;
            }
        }
        Ok(tools)
    }

    async fn call_tool(&self, name: &str, args: serde_json::Value) -> Result<serde_json::Value> {
        let params = json!({
            "name": name,
            "arguments": args
        });
        let result = self.request("tools/call", params).await?;
        Ok(result)
    }

    async fn request(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);

        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        self.send_message(&msg).await?;

        let response = rx.await
            .map_err(|_| anyhow::anyhow!("MCP server '{}' closed", self.name))?;

        if let Some(err) = response.get("error") {
            let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            bail!("MCP error from '{}': {}", self.name, msg);
        }

        Ok(response.get("result").cloned().unwrap_or(serde_json::Value::Null))
    }

    async fn notify(&self, method: &str, params: serde_json::Value) -> Result<()> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        self.send_message(&msg).await
    }

    async fn send_message(&self, msg: &serde_json::Value) -> Result<()> {
        let line = serde_json::to_string(msg)?;
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok(())
    }
}

fn start_stdout_reader(
    stdout: ChildStdout,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>>,
    name: String,
) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            let parsed: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if let Some(id) = parsed.get("id").and_then(|v| v.as_u64()) {
                if let Some(tx) = pending.lock().unwrap().remove(&id) {
                    let _ = tx.send(parsed);
                }
            } else {
                // Ignore notifications for now.
                let _ = name.as_str();
            }
        }
    });
}

fn parse_mcp_dependencies(config: &AgentConfig) -> Result<Vec<McpDependency>> {
    let deps_value = match &config.dependencies {
        Some(v) => v.clone(),
        None => return Ok(Vec::new()),
    };
    let deps: DependenciesConfig = serde_yaml::from_value(deps_value)
        .context("Invalid dependencies.mcp in agent.yaml")?;
    Ok(deps.mcp.unwrap_or_default())
}

#[derive(Debug, Deserialize, Default, Clone)]
struct DependenciesConfig {
    mcp: Option<Vec<McpDependency>>,
}

fn dependency_to_server(dep: &McpDependency) -> Option<McpServerConfig> {
    let command = dep.command.clone()?;
    Some(McpServerConfig {
        name: dep.name.clone(),
        transport: dep.transport.clone(),
        command,
        args: dep.args.clone(),
        env: dep.env.clone(),
        cwd: dep.cwd.clone(),
    })
}
