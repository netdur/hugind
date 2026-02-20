use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tokio::io::{self, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::mpsc;

use crate::core::config::settings::GlobalSettings;
use crate::core::model::downloader::{Downloader, ProgressSink};
use crate::core::model::registry::RepoManager;
use crate::core::model::remote::RemoteClient;
use crate::core::sys::{SystemInfo, SystemInspector};
use crate::shared::stdio::PrintSink;
use crate::shared::{configs, paths};

const SCHEMA_VERSION: &str = "1";

#[derive(Debug, Deserialize)]
struct Request {
    id: String,
    method: String,
    #[serde(default)]
    params: Option<JsonValue>,
}

#[derive(Debug, Deserialize)]
struct McpRequest {
    jsonrpc: Option<String>,
    id: Option<JsonValue>,
    method: String,
    #[serde(default)]
    params: Option<JsonValue>,
}

#[derive(Debug, Serialize)]
struct Response<T: Serialize> {
    id: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ErrorPayload>,
    schema_version: &'static str,
}

#[derive(Debug, Serialize)]
struct ErrorPayload {
    code: String,
    message: String,
}

#[derive(Debug, Serialize)]
struct Event<T: Serialize> {
    event: String,
    id: String,
    data: T,
    schema_version: &'static str,
}

#[derive(Clone)]
struct Outbox {
    tx: mpsc::UnboundedSender<JsonValue>,
}

impl Outbox {
    fn send<T: Serialize>(&self, msg: T) {
        let value = serde_json::to_value(msg).unwrap_or(JsonValue::Null);
        let _ = self.tx.send(value);
    }
}

pub async fn run() -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<JsonValue>();

    let stdout = init_stdout()?;
    let mut writer = BufWriter::new(stdout);
    let writer_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let line = serde_json::to_string(&msg).unwrap_or_else(|_| "{}".to_string());
            if writer.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            if writer.write_all(b"\n").await.is_err() {
                break;
            }
            let _ = writer.flush().await;
        }
    });

    let outbox = Outbox { tx };
    let stdin = BufReader::new(io::stdin());
    let mut lines = stdin.lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let value: JsonValue = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let err = Response::<JsonValue> {
                    id: "unknown".to_string(),
                    ok: false,
                    result: None,
                    error: Some(ErrorPayload {
                        code: "invalid_json".to_string(),
                        message: e.to_string(),
                    }),
                    schema_version: SCHEMA_VERSION,
                };
                outbox.send(err);
                continue;
            }
        };

        if is_mcp_message(&value) {
            if let Err(e) = handle_mcp_message(value, outbox.clone()).await {
                let err = mcp_error_response(
                    JsonValue::String("unknown".to_string()),
                    -32603,
                    e.to_string(),
                );
                outbox.send(err);
            }
            continue;
        }

        let req: Request = match serde_json::from_value(value) {
            Ok(r) => r,
            Err(e) => {
                let err = Response::<JsonValue> {
                    id: "unknown".to_string(),
                    ok: false,
                    result: None,
                    error: Some(ErrorPayload {
                        code: "invalid_request".to_string(),
                        message: e.to_string(),
                    }),
                    schema_version: SCHEMA_VERSION,
                };
                outbox.send(err);
                continue;
            }
        };

        let outbox = outbox.clone();
        let req_id = req.id.clone();
        if let Err(e) = handle_request(req, outbox.clone()).await {
            let err = Response::<JsonValue> {
                id: req_id,
                ok: false,
                result: None,
                error: Some(ErrorPayload {
                    code: "internal_error".to_string(),
                    message: e.to_string(),
                }),
                schema_version: SCHEMA_VERSION,
            };
            outbox.send(err);
        }
    }

    let _ = writer_task.await;
    Ok(())
}

async fn handle_request(req: Request, outbox: Outbox) -> Result<()> {
    let result = dispatch_method(&req.method, req.params, EventMode::Stdio { id: req.id.clone(), outbox: outbox.clone() }).await;
    match result {
        Ok(value) => {
            outbox.send(Response {
                id: req.id,
                ok: true,
                result: Some(value),
                error: None,
                schema_version: SCHEMA_VERSION,
            });
        }
        Err(e) => {
            outbox.send(Response::<JsonValue> {
                id: req.id,
                ok: false,
                result: None,
                error: Some(ErrorPayload {
                    code: "internal_error".to_string(),
                    message: e.to_string(),
                }),
                schema_version: SCHEMA_VERSION,
            });
        }
    }
    Ok(())
}

async fn dispatch_method(method: &str, params: Option<JsonValue>, mode: EventMode) -> Result<JsonValue> {
    match method {
        "agent.list" => {
            let agents = list_agents()?;
            Ok(serde_json::to_value(AgentListResult { agents })?)
        }
        "agent.remove" => {
            let params: AgentRemoveParams = parse_params(params)?;
            let result = remove_agent(&params.name)?;
            Ok(serde_json::to_value(result)?)
        }
        "agent.install" => {
            let params: AgentInstallParams = parse_params(params)?;
            let result = install_agent(&params)?;
            Ok(serde_json::to_value(result)?)
        }
        "agent.run" => {
            let params: AgentRunParams = parse_params(params)?;
            let result = match mode {
                EventMode::Stdio { id, outbox } => {
                    let emitter = std::sync::Arc::new(StdioEmitter::new(id, outbox));
                    run_agent_with_emitter(params, emitter).await?
                }
                EventMode::Mcp { id, outbox } => {
                    let emitter = std::sync::Arc::new(McpEmitter::new(id, outbox));
                    run_agent_with_emitter(params, emitter).await?
                }
            };
            Ok(serde_json::to_value(result)?)
        }
        "config.list" => {
            let configs = list_configs()?;
            Ok(serde_json::to_value(ConfigListResult { configs })?)
        }
        "config.validate" => {
            let params: ConfigValidateParams = parse_params(params)?;
            let result = validate_config(params.path)?;
            Ok(serde_json::to_value(result)?)
        }
        "config.info" => {
            let info = SystemInspector::inspect();
            Ok(serde_json::to_value(info)?)
        }
        "config.remove" => {
            let params: ConfigRemoveParams = parse_params(params)?;
            let result = remove_config(&params)?;
            Ok(serde_json::to_value(result)?)
        }
        "config.defaults" => {
            let params: ConfigDefaultsParams = parse_params(params)?;
            let result = config_defaults(params)?;
            Ok(serde_json::to_value(result)?)
        }
        "config.init" => {
            let params: ConfigInitParams = parse_params(params)?;
            let result = config_init(params)?;
            Ok(serde_json::to_value(result)?)
        }
        "model.list" => {
            let repos = list_models()?;
            Ok(serde_json::to_value(ModelListResult { repos })?)
        }
        "model.show" => {
            let params: ModelShowParams = parse_params(params)?;
            let result = show_model(params.repo)?;
            Ok(serde_json::to_value(result)?)
        }
        "model.add" => {
            let params: ModelAddParams = parse_params(params)?;
            let result = match mode {
                EventMode::Stdio { id, outbox } => {
                    let emitter = StdioEmitter::new(id, outbox);
                    add_model_with_emitter(params, &emitter).await?
                }
                EventMode::Mcp { id, outbox } => {
                    let emitter = McpEmitter::new(id, outbox);
                    add_model_with_emitter(params, &emitter).await?
                }
            };
            Ok(serde_json::to_value(result)?)
        }
        "model.remove" => {
            let params: ModelRemoveParams = parse_params(params)?;
            let result = remove_model(params)?;
            Ok(serde_json::to_value(result)?)
        }
        "server.list" => {
            let servers = list_servers().await?;
            Ok(serde_json::to_value(ServerListResult { servers })?)
        }
        "server.stop" => {
            let params: ServerStopParams = parse_params(params)?;
            let result = stop_server(params)?;
            Ok(serde_json::to_value(result)?)
        }
        "server.start" => {
            let params: ServerStartParams = parse_params(params)?;
            let result = start_server(params)?;
            Ok(serde_json::to_value(result)?)
        }
        _ => Err(anyhow!("Unknown method {}", method)),
    }
}

fn parse_params<T: for<'de> Deserialize<'de>>(params: Option<JsonValue>) -> Result<T> {
    let value = params.ok_or_else(|| anyhow!("Missing params"))?;
    serde_json::from_value(value).context("Invalid params")
}

fn is_mcp_message(value: &JsonValue) -> bool {
    value
        .get("jsonrpc")
        .and_then(|v| v.as_str())
        .map(|v| v == "2.0")
        .unwrap_or(false)
}

async fn handle_mcp_message(value: JsonValue, outbox: Outbox) -> Result<()> {
    let req: McpRequest = serde_json::from_value(value).context("Invalid MCP request")?;
    if req.jsonrpc.as_deref() != Some("2.0") {
        outbox.send(mcp_error_response(
            req.id.unwrap_or(JsonValue::Null),
            -32600,
            "Invalid Request: jsonrpc must be '2.0'".to_string(),
        ));
        return Ok(());
    }
    let Some(method) = Some(req.method.as_str()) else {
        return Ok(());
    };

    match method {
        "initialize" => {
            let id = req.id.unwrap_or(JsonValue::Null);
            let result = json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": { "name": "hugind", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": { "tools": {} }
            });
            outbox.send(mcp_response(id, result));
        }
        "notifications/initialized" => {}
        "tools/list" => {
            let id = req.id.unwrap_or(JsonValue::Null);
            let tools = mcp_tools();
            let result = json!({
                "tools": tools,
                "nextCursor": JsonValue::Null
            });
            outbox.send(mcp_response(id, result));
        }
        "tools/call" => {
            let id = req.id.unwrap_or(JsonValue::Null);
            let params = req.params.unwrap_or(JsonValue::Null);
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(JsonValue::Object(Default::default()));
            if name.is_empty() {
                outbox.send(mcp_error_response(id, -32602, "Missing tool name".to_string()));
                return Ok(());
            }
            match dispatch_method(name, Some(args), EventMode::Mcp { id: id.clone(), outbox: outbox.clone() }).await {
                Ok(result) => {
                    let content = json!([{
                        "type": "text",
                        "text": serde_json::to_string(&result).unwrap_or_else(|_| "null".to_string())
                    }]);
                    outbox.send(mcp_response(id, json!({ "content": content, "isError": false })));
                }
                Err(e) => {
                    outbox.send(mcp_error_response(id, -32603, e.to_string()));
                }
            }
        }
        _ => {
            if let Some(id) = req.id {
                outbox.send(mcp_error_response(id, -32601, format!("Unknown method {}", method)));
            }
        }
    }
    Ok(())
}

fn mcp_response(id: JsonValue, result: JsonValue) -> JsonValue {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn mcp_error_response(id: JsonValue, code: i64, message: String) -> JsonValue {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

fn mcp_tools() -> Vec<JsonValue> {
    vec![
        json!({"name":"agent.list","description":"List installed agents","inputSchema":empty_schema()}),
        json!({"name":"agent.run","description":"Run an agent or workflow","inputSchema":json!({"type":"object","properties":{"path":{"type":"string"},"args":{"type":"array","items":{"type":"string"}}},"required":["path"]})}),
        json!({"name":"agent.install","description":"Install an agent from a path or URL","inputSchema":json!({"type":"object","properties":{"path":{"type":"string"},"approve_permissions":{"type":"boolean"},"overwrite":{"type":"boolean"}},"required":["path","approve_permissions","overwrite"]})}),
        json!({"name":"agent.remove","description":"Remove an installed agent","inputSchema":json!({"type":"object","properties":{"name":{"type":"string"}},"required":["name"]})}),
        json!({"name":"config.list","description":"List configs","inputSchema":empty_schema()}),
        json!({"name":"config.validate","description":"Validate a config file","inputSchema":json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]})}),
        json!({"name":"config.info","description":"Inspect system info","inputSchema":empty_schema()}),
        json!({"name":"config.remove","description":"Remove a config","inputSchema":json!({"type":"object","properties":{"name":{"type":"string"},"confirm":{"type":"boolean"}},"required":["name","confirm"]})}),
        json!({"name":"config.defaults","description":"Get or set global defaults","inputSchema":json!({"type":"object","properties":{"lib":{"type":"string"},"hf_token":{"type":"string"}}})}),
        json!({"name":"config.init","description":"Create a config from explicit params","inputSchema":json!({"type":"object","properties":{"name":{"type":"string"},"model_path":{"type":"string"},"preset":{"type":"string"},"ctx":{"type":"number"},"mmproj_path":{"type":"string"},"format":{"type":"string"},"overwrite":{"type":"boolean"}},"required":["name","model_path"]})}),
        json!({"name":"model.list","description":"List downloaded model repos","inputSchema":empty_schema()}),
        json!({"name":"model.show","description":"Show files for a model repo","inputSchema":json!({"type":"object","properties":{"repo":{"type":"string"}},"required":["repo"]})}),
        json!({"name":"model.add","description":"Download model files from a repo","inputSchema":json!({"type":"object","properties":{"repo":{"type":"string"},"files":{"type":"array","items":{"type":"string"}}},"required":["repo","files"]})}),
        json!({"name":"model.remove","description":"Remove model files or repos","inputSchema":json!({"type":"object","properties":{"repo":{"type":"string"},"files":{"type":"array","items":{"type":"string"}},"delete_repo":{"type":"boolean"},"delete_if_empty":{"type":"boolean"}},"required":["repo"]})}),
        json!({"name":"server.list","description":"List server configs and status","inputSchema":empty_schema()}),
        json!({"name":"server.start","description":"Start a server for a config","inputSchema":json!({"type":"object","properties":{"config":{"type":"string"},"port":{"type":"number"}},"required":["config"]})}),
        json!({"name":"server.stop","description":"Stop a server for a config","inputSchema":json!({"type":"object","properties":{"config":{"type":"string"}},"required":["config"]})}),
    ]
}

fn empty_schema() -> JsonValue {
    json!({"type":"object","properties":{}})
}

type StdoutWriter = Box<dyn AsyncWrite + Unpin + Send>;

#[cfg(unix)]
fn init_stdout() -> Result<StdoutWriter> {
    use std::os::unix::io::{AsRawFd, FromRawFd};
    let stdout_fd = std::io::stdout().as_raw_fd();
    let stderr_fd = std::io::stderr().as_raw_fd();
    let dup_fd = unsafe { libc::dup(stdout_fd) };
    if dup_fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let res = unsafe { libc::dup2(stderr_fd, stdout_fd) };
    if res < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let file = unsafe { std::fs::File::from_raw_fd(dup_fd) };
    Ok(Box::new(tokio::fs::File::from_std(file)))
}

#[cfg(not(unix))]
fn init_stdout() -> Result<StdoutWriter> {
    Ok(Box::new(tokio::io::stdout()))
}

#[derive(Clone)]
struct StdioEmitter {
    id: String,
    outbox: Outbox,
}

#[derive(Clone)]
struct McpEmitter {
    id: JsonValue,
    outbox: Outbox,
}

trait StatusEmitter: Send + Sync {
    fn status(&self, message: &str);
}

enum EventMode {
    Stdio { id: String, outbox: Outbox },
    Mcp { id: JsonValue, outbox: Outbox },
}

impl StdioEmitter {
    fn new(id: String, outbox: Outbox) -> Self {
        Self { id, outbox }
    }

    fn progress(&self, repo: &str, filename: &str, downloaded: u64, total: Option<u64>) {
        let event = Event {
            event: "progress".to_string(),
            id: self.id.clone(),
            data: ProgressEvent {
                repo: repo.to_string(),
                file: filename.to_string(),
                downloaded,
                total,
            },
            schema_version: SCHEMA_VERSION,
        };
        self.outbox.send(event);
    }

    fn log(&self, message: impl Into<String>) {
        let event = Event {
            event: "log".to_string(),
            id: self.id.clone(),
            data: LogEvent { message: message.into() },
            schema_version: SCHEMA_VERSION,
        };
        self.outbox.send(event);
    }
}

impl McpEmitter {
    fn new(id: JsonValue, outbox: Outbox) -> Self {
        Self { id, outbox }
    }

    fn notify(&self, method: &str, params: JsonValue) {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        self.outbox.send(msg);
    }

    fn progress(&self, repo: &str, filename: &str, downloaded: u64, total: Option<u64>) {
        self.notify(
            "notifications/hugind.progress",
            json!({
                "id": self.id,
                "repo": repo,
                "file": filename,
                "downloaded": downloaded,
                "total": total
            }),
        );
    }

    fn log(&self, message: impl Into<String>) {
        self.notify(
            "notifications/hugind.log",
            json!({
                "id": self.id,
                "message": message.into()
            }),
        );
    }
}

impl StatusEmitter for StdioEmitter {
    fn status(&self, message: &str) {
        let event = Event {
            event: "status".to_string(),
            id: self.id.clone(),
            data: StatusEvent { message: message.to_string() },
            schema_version: SCHEMA_VERSION,
        };
        self.outbox.send(event);
    }
}

impl StatusEmitter for McpEmitter {
    fn status(&self, message: &str) {
        self.notify(
            "notifications/hugind.status",
            json!({
                "id": self.id,
                "message": message
            }),
        );
    }
}

impl ProgressSink for StdioEmitter {
    fn on_start(&self, repo: &str, filename: &str, total_bytes: Option<u64>) {
        self.status(&format!("download.start repo={} file={}", repo, filename));
        self.progress(repo, filename, 0, total_bytes);
    }

    fn on_progress(&self, repo: &str, filename: &str, downloaded: u64, total_bytes: Option<u64>) {
        self.progress(repo, filename, downloaded, total_bytes);
    }

    fn on_finish(&self, repo: &str, filename: &str, _final_path: &PathBuf) {
        self.status(&format!("download.finish repo={} file={}", repo, filename));
    }
}

impl ProgressSink for McpEmitter {
    fn on_start(&self, repo: &str, filename: &str, total_bytes: Option<u64>) {
        self.status(&format!("download.start repo={} file={}", repo, filename));
        self.progress(repo, filename, 0, total_bytes);
    }

    fn on_progress(&self, repo: &str, filename: &str, downloaded: u64, total_bytes: Option<u64>) {
        self.progress(repo, filename, downloaded, total_bytes);
    }

    fn on_finish(&self, repo: &str, filename: &str, _final_path: &PathBuf) {
        self.status(&format!("download.finish repo={} file={}", repo, filename));
    }
}

impl PrintSink for StdioEmitter {
    fn print(&self, msg: &str) {
        self.log(msg.to_string());
    }

    fn print_raw(&self, msg: &str) {
        self.log(msg.to_string());
    }
}

impl PrintSink for McpEmitter {
    fn print(&self, msg: &str) {
        self.log(msg.to_string());
    }

    fn print_raw(&self, msg: &str) {
        self.log(msg.to_string());
    }
}

#[derive(Debug, Serialize)]
struct StatusEvent {
    message: String,
}

#[derive(Debug, Serialize)]
struct ProgressEvent {
    repo: String,
    file: String,
    downloaded: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    total: Option<u64>,
}

#[derive(Debug, Serialize)]
struct LogEvent {
    message: String,
}

#[derive(Debug, Serialize)]
struct AgentListResult {
    agents: Vec<AgentListItem>,
}

#[derive(Debug, Serialize)]
struct AgentListItem {
    name: String,
    path: String,
}

#[derive(Debug, Deserialize)]
struct AgentRemoveParams {
    name: String,
}

#[derive(Debug, Serialize)]
struct AgentRemoveResult {
    name: String,
    path: String,
    removed: bool,
}

#[derive(Debug, Deserialize)]
struct AgentInstallParams {
    path: String,
    approve_permissions: bool,
    overwrite: bool,
}

#[derive(Debug, Serialize)]
struct AgentInstallResult {
    name: String,
    path: String,
    permissions: PermissionsSummary,
}

#[derive(Debug, Serialize)]
struct PermissionsSummary {
    network: PermissionSummary,
    filesystem: PermissionSummary,
    shell: PermissionSummary,
}

#[derive(Debug, Serialize)]
struct PermissionSummary {
    allow: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    details: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AgentRunParams {
    path: String,
    #[serde(default)]
    args: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AgentRunResult {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<JsonValue>,
}

fn list_agents() -> Result<Vec<AgentListItem>> {
    let dir = paths::agents_dir();
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut items = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                items.push(AgentListItem {
                    name: name.to_string(),
                    path: dir.join(name).to_string_lossy().to_string(),
                });
            }
        }
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(items)
}

fn remove_agent(name: &str) -> Result<AgentRemoveResult> {
    let dir = paths::agents_dir();
    let target = dir.join(name);
    if !target.exists() {
        return Err(anyhow!("Agent '{}' not found", name));
    }
    std::fs::remove_dir_all(&target)?;
    Ok(AgentRemoveResult {
        name: name.to_string(),
        path: target.to_string_lossy().to_string(),
        removed: true,
    })
}

async fn run_agent_with_emitter<E>(params: AgentRunParams, emitter: std::sync::Arc<E>) -> Result<AgentRunResult>
where
    E: PrintSink + StatusEmitter + Send + Sync + 'static,
{
    let _guard = agent_run_lock().lock().await;
    let sink: std::sync::Arc<dyn PrintSink> = emitter.clone();
    let _sink_guard = PrintSinkGuard::new(sink);

    emitter.status("agent.run.start");
    let result = crate::core::orchestrator::execute_with_result(
        params.path,
        params.args,
        None,
        None,
    )
    .await?;
    emitter.status("agent.run.finish");
    Ok(AgentRunResult {
        status: "ok".to_string(),
        result: Some(result),
    })
}

fn agent_run_lock() -> &'static tokio::sync::Mutex<()> {
    use std::sync::OnceLock;
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

struct PrintSinkGuard;

impl PrintSinkGuard {
    fn new(sink: std::sync::Arc<dyn PrintSink>) -> Self {
        crate::shared::stdio::set_print_sink(Some(sink));
        Self
    }
}

impl Drop for PrintSinkGuard {
    fn drop(&mut self) {
        crate::shared::stdio::set_print_sink(None);
    }
}

fn install_agent(params: &AgentInstallParams) -> Result<AgentInstallResult> {
    let (source_root, config, _temp_guard) = if is_url(&params.path) {
        if is_zip_path(&params.path) {
            download_zip_agent(&params.path)?
        } else {
            download_agent(&params.path)?
        }
    } else if is_zip_path(&params.path) {
        let root = extract_local_zip_agent(&params.path)?;
        let config = crate::core::config::agent::AgentConfig::load_from_dir(&root)?;
        (root, config, None)
    } else {
        let root = resolve_local_agent_root(&params.path)?;
        let config = crate::core::config::agent::AgentConfig::load_from_dir(&root)?;
        (root, config, None)
    };

    let permissions = summarize_permissions(&config.permissions);
    if !params.approve_permissions {
        return Err(anyhow!("Permissions not approved"));
    }

    let dest_dir = paths::agents_dir().join(sanitize_agent_name(&config.name));
    if dest_dir.exists() {
        if !params.overwrite {
            return Err(anyhow!("Agent already exists at {}", dest_dir.display()));
        }
        std::fs::remove_dir_all(&dest_dir)?;
    }
    std::fs::create_dir_all(&dest_dir)?;
    copy_dir_recursive(&source_root, &dest_dir)?;

    Ok(AgentInstallResult {
        name: config.name,
        path: dest_dir.to_string_lossy().to_string(),
        permissions,
    })
}

fn summarize_permissions(perms: &Option<crate::core::config::agent::Permissions>) -> PermissionsSummary {
    let mut network = PermissionSummary { allow: false, details: Vec::new() };
    let mut filesystem = PermissionSummary { allow: false, details: Vec::new() };
    let mut shell = PermissionSummary { allow: false, details: Vec::new() };

    if let Some(perms) = perms {
        if let Some(net) = &perms.network {
            network.allow = net.allow;
            if !net.allowed_domains.is_empty() {
                network.details.push(format!("domains: {}", net.allowed_domains.join(", ")));
            }
            if !net.allowed_ips.is_empty() {
                network.details.push(format!("ips: {}", net.allowed_ips.join(", ")));
            }
            if net.block_private_networks {
                network.details.push("blocks private networks".to_string());
            }
            if let Some(v) = &net.max_response_bytes {
                network.details.push(format!("max response: {}", v));
            }
            if let Some(v) = &net.timeout {
                network.details.push(format!("timeout: {}", v));
            }
        }

        if let Some(fs_perm) = &perms.filesystem {
            filesystem.allow = fs_perm.allow;
            let mut actions = Vec::new();
            if fs_perm.read { actions.push("read"); }
            if fs_perm.write { actions.push("write"); }
            if fs_perm.create { actions.push("create"); }
            if fs_perm.delete { actions.push("delete"); }
            if !actions.is_empty() {
                filesystem.details.push(format!("actions: {}", actions.join(", ")));
            }
            if !fs_perm.allowed_paths.is_empty() {
                filesystem.details.push(format!("paths: {}", fs_perm.allowed_paths.join(", ")));
            }
            if !fs_perm.denied_paths.is_empty() {
                filesystem.details.push(format!("blocked: {}", fs_perm.denied_paths.join(", ")));
            }
            if fs_perm.allow_outside_agent_root {
                filesystem.details.push("can access outside agent folder".to_string());
            }
            if fs_perm.follow_symlinks {
                filesystem.details.push("follows symlinks".to_string());
            }
        }

        if let Some(shell_perm) = &perms.shell {
            shell.allow = shell_perm.allow;
            if let Some(list) = &shell_perm.whitelist {
                if !list.is_empty() {
                    shell.details.push(format!("allowed: {}", list.join(", ")));
                }
            }
            if let Some(list) = &shell_perm.blacklist {
                if !list.is_empty() {
                    shell.details.push(format!("blocked: {}", list.join(", ")));
                }
            }
            if let Some(v) = &shell_perm.timeout {
                shell.details.push(format!("timeout: {}", v));
            }
            if let Some(v) = &shell_perm.max_output {
                shell.details.push(format!("max output: {}", v));
            }
            if shell_perm.env_clear {
                shell.details.push("clears env".to_string());
            }
            if let Some(v) = &shell_perm.working_dir {
                shell.details.push(format!("working dir: {}", v));
            }
        }
    }

    PermissionsSummary { network, filesystem, shell }
}

#[derive(Debug, Serialize)]
struct ConfigListResult {
    configs: Vec<ConfigListItem>,
}

#[derive(Debug, Serialize)]
struct ConfigListItem {
    name: String,
    path: String,
}

fn list_configs() -> Result<Vec<ConfigListItem>> {
    let config_dir = paths::configs_dir();
    if !config_dir.exists() {
        return Ok(vec![]);
    }
    let mut items = Vec::new();
    for entry in std::fs::read_dir(&config_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if ext == "yml" || ext == "yaml" {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if configs::is_reserved_config_name(stem) {
                            continue;
                        }
                        items.push(ConfigListItem {
                            name: stem.to_string(),
                            path: path.to_string_lossy().to_string(),
                        });
                    }
                }
            }
        }
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(items)
}

#[derive(Debug, Deserialize)]
struct ConfigValidateParams {
    path: String,
}

#[derive(Debug, Serialize)]
struct ConfigValidateResult {
    valid: bool,
}

fn validate_config(path: String) -> Result<ConfigValidateResult> {
    let config_path = Path::new(&path);
    if !config_path.exists() {
        return Err(anyhow!("Config file not found: {}", path));
    }
    crate::core::config::loader::ConfigLoader::load_server_config(config_path)?;
    Ok(ConfigValidateResult { valid: true })
}

#[derive(Debug, Deserialize)]
struct ConfigRemoveParams {
    name: String,
    confirm: bool,
}

#[derive(Debug, Serialize)]
struct ConfigRemoveResult {
    removed: bool,
    path: Option<String>,
}

fn remove_config(params: &ConfigRemoveParams) -> Result<ConfigRemoveResult> {
    if !params.confirm {
        return Err(anyhow!("Removal not confirmed"));
    }
    let config_dir = paths::configs_dir();
    let yml_path = config_dir.join(format!("{}.yml", params.name));
    let yaml_path = config_dir.join(format!("{}.yaml", params.name));
    let path_to_remove = if yml_path.exists() {
        Some(yml_path)
    } else if yaml_path.exists() {
        Some(yaml_path)
    } else {
        None
    };
    if let Some(p) = path_to_remove {
        std::fs::remove_file(&p)?;
        Ok(ConfigRemoveResult {
            removed: true,
            path: Some(p.to_string_lossy().to_string()),
        })
    } else {
        Ok(ConfigRemoveResult {
            removed: false,
            path: None,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ConfigDefaultsParams {
    #[serde(default)]
    lib: Option<String>,
    #[serde(default)]
    hf_token: Option<String>,
}

#[derive(Debug, Serialize)]
struct ConfigDefaultsResult {
    settings: Vec<ConfigSettingItem>,
}

#[derive(Debug, Serialize)]
struct ConfigSettingItem {
    key: String,
    value: String,
}

fn config_defaults(params: ConfigDefaultsParams) -> Result<ConfigDefaultsResult> {
    ensure_settings_file()?;
    let mut settings = GlobalSettings::load()?;
    if let Some(l) = params.lib {
        settings.set("library_path", &l);
    }
    if let Some(t) = params.hf_token {
        settings.set("hf_token", &t);
    }
    settings.save()?;
    let mut items = Vec::new();
    for (k, v) in &settings.0 {
        items.push(ConfigSettingItem {
            key: k.to_string(),
            value: v.to_string(),
        });
    }
    Ok(ConfigDefaultsResult { settings: items })
}

fn ensure_settings_file() -> Result<()> {
    let path = paths::data_home().join("settings.yml");
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = include_str!("../../assets/settings.yml");
    std::fs::write(&path, content)?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ConfigInitParams {
    name: String,
    model_path: String,
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    ctx: Option<u64>,
    #[serde(default)]
    mmproj_path: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Debug, Serialize)]
struct ConfigInitResult {
    path: String,
    preset: String,
    model_path: String,
    ctx: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    mmproj_path: Option<String>,
}

fn config_init(params: ConfigInitParams) -> Result<ConfigInitResult> {
    ensure_default_configs()?;
    let info = SystemInspector::inspect();
    let recommended_preset = SystemInspector::recommend_preset(&info);
    let chosen_preset = params
        .preset
        .unwrap_or_else(|| recommended_preset.to_string());

    let base_content = include_str!("../resources/config.yml");
    let preset_content = match chosen_preset.as_str() {
        "metal_unified" => include_str!("../resources/metal_unified.yml"),
        "cuda_dedicated" => include_str!("../resources/cuda_dedicated.yml"),
        "cpu_only" => include_str!("../resources/cpu_only.yml"),
        _ => return Err(anyhow!("Unknown preset {}", chosen_preset)),
    };

    let model_path = params.model_path;
    if !Path::new(&model_path).exists() {
        return Err(anyhow!("Model path not found: {}", model_path));
    }
    let model_size_gb = if let Ok(meta) = std::fs::metadata(&model_path) {
        meta.len() as f64 / 1_073_741_824.0
    } else {
        0.0
    };

    let mmproj_path = params.mmproj_path.or_else(|| detect_sibling(&model_path, &["mmproj", "projector", "vision"]));
    let chosen_format = params.format.unwrap_or_else(|| "auto".to_string());

    let final_ctx = params.ctx.unwrap_or_else(|| recommend_ctx(&info, model_size_gb));

    let mut final_content = base_content.to_string();
    for line in preset_content.lines() {
        if let Some((k, v)) = parse_yaml_line(line) {
            final_content = replace_value(&final_content, &k, &v);
        }
    }

    final_content = replace_value(&final_content, "path", &format!("\"{}\"", shorten_path(&model_path)));
    if let Some(mm) = &mmproj_path {
        final_content = replace_value(&final_content, "mmproj_path", &format!("\"{}\"", shorten_path(mm)));
        final_content = replace_value(&final_content, "batch_size", "8192");
    }
    let unified_memory_mode = chosen_preset == "metal_unified";
    final_content = replace_value(&final_content, "unified_memory_mode", if unified_memory_mode { "true" } else { "false" });
    final_content = replace_value(&final_content, "format", &chosen_format);
    final_content = replace_value(&final_content, "size", &final_ctx.to_string());

    let dest_dir = paths::configs_dir();
    std::fs::create_dir_all(&dest_dir)?;
    let dest_file = dest_dir.join(format!("{}.yml", params.name));
    if dest_file.exists() && !params.overwrite {
        return Err(anyhow!("Config '{}' already exists", params.name));
    }
    std::fs::write(&dest_file, final_content)?;

    Ok(ConfigInitResult {
        path: dest_file.to_string_lossy().to_string(),
        preset: chosen_preset,
        model_path: shorten_path(&model_path),
        ctx: final_ctx,
        mmproj_path: mmproj_path.as_deref().map(shorten_path),
    })
}

fn ensure_default_configs() -> Result<()> {
    let dest_dir = paths::presets_dir();
    std::fs::create_dir_all(&dest_dir)?;
    let defaults = [
        ("config.yml", include_str!("../resources/config.yml")),
        ("cpu_only.yml", include_str!("../resources/cpu_only.yml")),
        ("cuda_dedicated.yml", include_str!("../resources/cuda_dedicated.yml")),
        ("metal_unified.yml", include_str!("../resources/metal_unified.yml")),
    ];
    for (name, content) in defaults {
        let path = dest_dir.join(name);
        if path.exists() {
            continue;
        }
        std::fs::write(&path, content)?;
    }
    Ok(())
}

fn recommend_ctx(info: &SystemInfo, model_size_gb: f64) -> u64 {
    let sys_mem_gb = info.memory_bytes as f64 / 1_073_741_824.0;
    let available_for_ctx = (sys_mem_gb - model_size_gb - 2.0).max(0.5);
    let est_tokens = (available_for_ctx * 10.0 * 1024.0) as u64;

    let mut ctx_options = vec![2048, 4096, 8192, 16384, 32768, 65536];
    let mut next_ctx = 131072u64;
    let max_ctx = est_tokens.min(262_144);
    while next_ctx <= max_ctx {
        ctx_options.push(next_ctx);
        next_ctx *= 2;
    }
    ctx_options
        .iter()
        .filter(|&&c| c <= est_tokens)
        .last()
        .copied()
        .unwrap_or(2048)
}

#[derive(Debug, Serialize)]
struct ModelListResult {
    repos: Vec<ModelRepoItem>,
}

#[derive(Debug, Serialize)]
struct ModelRepoItem {
    name: String,
    path: String,
}

fn list_models() -> Result<Vec<ModelRepoItem>> {
    let repos = RepoManager::list_repos()?;
    let mut items = Vec::new();
    for repo in repos {
        items.push(ModelRepoItem {
            name: repo.full_name(),
            path: repo.path.to_string_lossy().to_string(),
        });
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(items)
}

#[derive(Debug, Deserialize)]
struct ModelShowParams {
    repo: String,
}

#[derive(Debug, Serialize)]
struct ModelShowResult {
    repo: String,
    files: Vec<ModelFileItem>,
}

#[derive(Debug, Serialize)]
struct ModelFileItem {
    name: String,
    path: String,
    size_bytes: u64,
}

fn show_model(repo: String) -> Result<ModelShowResult> {
    if !RepoManager::repo_exists(&repo) {
        return Err(anyhow!("Repository '{}' not found", repo));
    }
    let repos = RepoManager::list_repos()?;
    let repo_obj = repos.iter().find(|r| r.full_name() == repo)
        .ok_or_else(|| anyhow!("Repository metadata not found"))?;
    let files = RepoManager::list_repo_files(repo_obj)?;
    let items = files.into_iter().map(|f| ModelFileItem {
        name: f.name,
        path: f.path.to_string_lossy().to_string(),
        size_bytes: f.size_bytes,
    }).collect();
    Ok(ModelShowResult { repo, files: items })
}

#[derive(Debug, Deserialize)]
struct ModelAddParams {
    repo: String,
    files: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ModelAddResult {
    repo: String,
    files: Vec<String>,
}

async fn add_model_with_emitter<E>(params: ModelAddParams, emitter: &E) -> Result<ModelAddResult>
where
    E: ProgressSink,
{
    if params.files.is_empty() {
        return Err(anyhow!("No files specified"));
    }

    let available = RemoteClient::fetch_repo_files(&params.repo).await
        .context("Failed to fetch remote files")?;
    let available_set: std::collections::HashSet<String> = available.into_iter().collect();

    for file in &params.files {
        if !available_set.contains(file) {
            return Err(anyhow!("File '{}' not found in repo {}", file, params.repo));
        }
    }

    for filename in &params.files {
        Downloader::download_file_with_sink(&params.repo, filename, Some(emitter)).await?;
    }

    Ok(ModelAddResult { repo: params.repo, files: params.files })
}

#[derive(Debug, Deserialize)]
struct ModelRemoveParams {
    repo: String,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    delete_repo: bool,
    #[serde(default)]
    delete_if_empty: bool,
}

#[derive(Debug, Serialize)]
struct ModelRemoveResult {
    repo: String,
    deleted_repo: bool,
    deleted_files: Vec<String>,
}

fn remove_model(params: ModelRemoveParams) -> Result<ModelRemoveResult> {
    if !RepoManager::repo_exists(&params.repo) {
        return Err(anyhow!("Repository '{}' does not exist locally", params.repo));
    }

    let mut deleted_files = Vec::new();
    if params.delete_repo {
        RepoManager::delete_repo(&params.repo)?;
        return Ok(ModelRemoveResult {
            repo: params.repo,
            deleted_repo: true,
            deleted_files,
        });
    }

    for filename in &params.files {
        RepoManager::delete_file(&params.repo, filename)?;
        deleted_files.push(filename.clone());
    }

    if params.delete_if_empty {
        let repos = RepoManager::list_repos()?;
        if let Some(repo_obj) = repos.iter().find(|r| r.full_name() == params.repo) {
            let remaining = RepoManager::list_repo_files(repo_obj)?;
            if remaining.is_empty() {
                RepoManager::delete_repo(&params.repo)?;
                return Ok(ModelRemoveResult {
                    repo: params.repo,
                    deleted_repo: true,
                    deleted_files,
                });
            }
        }
    }

    Ok(ModelRemoveResult {
        repo: params.repo,
        deleted_repo: false,
        deleted_files,
    })
}

#[derive(Debug, Serialize)]
struct ServerListResult {
    servers: Vec<ServerItem>,
}

#[derive(Debug, Serialize)]
struct ServerItem {
    name: String,
    host: String,
    port: u16,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ServerStopParams {
    config: String,
}

#[derive(Debug, Serialize)]
struct ServerStopResult {
    config: String,
    stopped: bool,
    pids: Vec<i32>,
}

#[derive(Debug, Deserialize)]
struct ServerStartParams {
    config: String,
    #[serde(default)]
    port: Option<u16>,
}

#[derive(Debug, Serialize)]
struct ServerStartResult {
    status: String,
}

fn start_server(params: ServerStartParams) -> Result<ServerStartResult> {
    let exe = std::env::current_exe().with_context(|| "Failed to resolve current executable path")?;
    let mut cmd = Command::new(exe);
    cmd.arg("server").arg("start").arg(params.config);
    if let Some(port) = params.port {
        cmd.arg("--port").arg(port.to_string());
    }
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    cmd.spawn().with_context(|| "Failed to spawn detached server process")?;
    Ok(ServerStartResult {
        status: "starting".to_string(),
    })
}

async fn list_servers() -> Result<Vec<ServerItem>> {
    let config_dir = paths::configs_dir();
    if !config_dir.exists() {
        return Ok(vec![]);
    }

    let configs = list_config_files(&config_dir)?;
    let mut servers = Vec::new();
    for path in configs {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
        let (host, port) = read_host_port(&path).unwrap_or(("127.0.0.1".to_string(), 8080));
        let monitor_url = format!("http://{}:{}/v1/monitor", normalize_host(&host), port);
        let info = fetch_monitor_info(&monitor_url).await;
        let status = format_status(info.as_ref(), name);
        servers.push(ServerItem {
            name: name.to_string(),
            host,
            port,
            status,
            config_name: info.and_then(|i| i.config_name),
        });
    }
    Ok(servers)
}

fn stop_server(params: ServerStopParams) -> Result<ServerStopResult> {
    let path = find_config_path(&params.config)
        .ok_or_else(|| anyhow!("Config '{}' not found", params.config))?;
    let (_host, port) = read_host_port(&path).unwrap_or(("127.0.0.1".to_string(), 8080));
    if cfg!(target_os = "windows") {
        return Ok(ServerStopResult {
            config: params.config,
            stopped: false,
            pids: Vec::new(),
        });
    }
    let pids = kill_by_port(port)?;
    Ok(ServerStopResult {
        config: params.config,
        stopped: !pids.is_empty(),
        pids,
    })
}

fn list_config_files(config_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut configs = Vec::new();
    for entry in std::fs::read_dir(config_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if ext == "yml" || ext == "yaml" {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if configs::is_reserved_config_name(stem) {
                            continue;
                        }
                    }
                    configs.push(path);
                }
            }
        }
    }
    Ok(configs)
}

fn find_config_path(config: &str) -> Option<PathBuf> {
    let config_dir = paths::configs_dir();
    let yml_path = config_dir.join(format!("{}.yml", config));
    let yaml_path = config_dir.join(format!("{}.yaml", config));
    if yml_path.exists() {
        Some(yml_path)
    } else if yaml_path.exists() {
        Some(yaml_path)
    } else {
        None
    }
}

fn read_host_port(path: &Path) -> Result<(String, u16)> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {:?}", path))?;
    let yaml: serde_yaml::Value = serde_yaml::from_str(&content).with_context(|| "Failed to parse YAML")?;
    let server = yaml.get("server").unwrap_or(&serde_yaml::Value::Null);
    let host = server
        .get("host")
        .and_then(|h| h.as_str())
        .unwrap_or("127.0.0.1")
        .to_string();
    let port = server
        .get("port")
        .and_then(|p| p.as_u64())
        .unwrap_or(8080) as u16;
    Ok((host, port))
}

fn normalize_host(host: &str) -> &str {
    if host == "0.0.0.0" {
        "127.0.0.1"
    } else {
        host
    }
}

fn kill_by_port(port: u16) -> Result<Vec<i32>> {
    use std::process::Command;
    let output = Command::new("lsof")
        .arg("-ti")
        .arg(format!("tcp:{}", port))
        .output()
        .with_context(|| "Failed to run lsof")?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let self_pid = std::process::id() as i32;
    let pids: Vec<i32> = stdout
        .lines()
        .filter_map(|line| line.trim().parse::<i32>().ok())
        .filter(|pid| *pid != self_pid)
        .collect();
    if pids.is_empty() {
        return Ok(Vec::new());
    }
    for pid in &pids {
        let _ = Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .status();
    }
    Ok(pids)
}

async fn fetch_monitor_info(url: &str) -> Option<MonitorInfo> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(1))
        .build()
        .ok()?;
    let resp = client.get(url).send().await.ok()?;
    let resp = resp.error_for_status().ok()?;
    let body = resp.text().await.ok()?;
    let json = serde_json::from_str::<JsonValue>(&body).ok()?;
    let config_name = json.get("config_name").and_then(|v| v.as_str()).map(|s| s.to_string());
    Some(MonitorInfo { config_name })
}

fn format_status(info: Option<&MonitorInfo>, expected: &str) -> String {
    let Some(info) = info else {
        return "down".to_string();
    };
    if let Some(config_name) = info.config_name.as_deref() {
        if config_name == expected {
            return "up".to_string();
        }
        return "down".to_string();
    }
    "up".to_string()
}

#[derive(Debug)]
struct MonitorInfo {
    config_name: Option<String>,
}

fn is_url(input: &str) -> bool {
    reqwest::Url::parse(input)
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
                .ok_or_else(|| anyhow!("Invalid agent.yaml path"))?
                .to_path_buf());
        }
        return Err(anyhow!("Expected a folder containing agent.yaml or a direct agent.yaml path"));
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

fn download_agent(path: &str) -> Result<(PathBuf, crate::core::config::agent::AgentConfig, Option<tempfile::TempDir>)> {
    let base_url = resolve_agent_base_url(path)?;
    let agent_url = base_url.join("agent.yaml")?;
    let temp = tempfile::tempdir()?;
    let root = temp.path().to_path_buf();
    let agent_yaml = reqwest::blocking::get(agent_url.clone())?.error_for_status()?.text()?;
    std::fs::write(root.join("agent.yaml"), agent_yaml)?;
    let config = crate::core::config::agent::AgentConfig::load_from_dir(&root)?;
    let entry_url = base_url.join(&config.entry_point)?;
    let entry_path = root.join(&config.entry_point);
    if let Some(parent) = entry_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let entry_bytes = reqwest::blocking::get(entry_url.clone())?.error_for_status()?.bytes()?;
    std::fs::write(&entry_path, &entry_bytes)?;
    Ok((root, config, Some(temp)))
}

fn download_zip_agent(path: &str) -> Result<(PathBuf, crate::core::config::agent::AgentConfig, Option<tempfile::TempDir>)> {
    let url = reqwest::Url::parse(path)?;
    let temp = tempfile::tempdir()?;
    let root = temp.path().to_path_buf();
    let zip_path = root.join("agent.zip");
    let bytes = reqwest::blocking::get(url.clone())?.error_for_status()?.bytes()?;
    std::fs::write(&zip_path, &bytes)?;
    extract_zip(&zip_path, &root)?;
    let agent_root = find_agent_root(&root)?;
    let config = crate::core::config::agent::AgentConfig::load_from_dir(&agent_root)?;
    Ok((agent_root, config, Some(temp)))
}

fn resolve_agent_base_url(path: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(path)?;
    if let Some(raw_base) = github_raw_base(&url) {
        return Ok(raw_base);
    }
    if path.ends_with("agent.yaml") {
        return Ok(url.join(".")?);
    }
    if path.ends_with('/') {
        return Ok(url);
    }
    reqwest::Url::parse(&(path.to_string() + "/")).map_err(Into::into)
}

fn github_raw_base(url: &reqwest::Url) -> Option<reqwest::Url> {
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
    let mut base = format!("https://raw.githubusercontent.com/{}/{}/{}/", owner, repo, branch);
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
    reqwest::Url::parse(&base).ok()
}

fn sanitize_agent_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "agent".to_string();
    }
    trimmed
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(zip_path)
        .with_context(|| format!("Failed to open zip {}", zip_path.display()))?;
    let mut archive = zip::read::ZipArchive::new(file)
        .with_context(|| format!("Invalid zip {}", zip_path.display()))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(rel_path) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest.join(rel_path);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut outfile = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut outfile)?;
    }
    Ok(())
}

fn find_agent_root(root: &Path) -> Result<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut found: Option<PathBuf> = None;
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
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
                        return Err(anyhow!(
                            "Multiple agent.yaml files found in zip; please provide a zip with a single agent"
                        ));
                    }
                } else {
                    found = Some(candidate);
                }
            }
        }
    }
    found.ok_or_else(|| anyhow!("agent.yaml not found in zip"))
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)
        .with_context(|| format!("Failed to create {}", dst.display()))?;
    for entry in std::fs::read_dir(src)
        .with_context(|| format!("Failed to read {}", src.display()))? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if ty.is_file() {
            std::fs::copy(&src_path, &dst_path)
                .with_context(|| format!("Failed to copy {} to {}", src_path.display(), dst_path.display()))?;
        }
    }
    Ok(())
}

fn detect_sibling(main_path: &str, keywords: &[&str]) -> Option<String> {
    let path = Path::new(main_path);
    if !path.exists() {
        return None;
    }
    let parent = path.parent()?;
    let main_name = path.file_name()?.to_str()?.to_lowercase();
    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.filter_map(Result::ok) {
            let p = entry.path();
            if p.is_file() {
                if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                    let name_lower = name.to_lowercase();
                    if name_lower != main_name && name_lower.ends_with(".gguf") {
                        if keywords.iter().any(|k| name_lower.contains(k)) {
                            return Some(p.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

fn shorten_path(path: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        if path.starts_with(home_str.as_ref()) {
            return path.replacen(home_str.as_ref(), "~", 1);
        }
    }
    path.to_string()
}

fn replace_value(content: &str, key: &str, new_value: &str) -> String {
    let mut output = String::new();
    let key_pat = format!("{}:", key);
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(&key_pat) {
            let indent = &line[0..line.len() - trimmed.len()];
            let parts: Vec<&str> = trimmed.splitn(2, ':').collect();
            if parts.len() == 2 {
                let rest = parts[1];
                let comment_idx = rest.find('#');
                let comment = if let Some(idx) = comment_idx {
                    &rest[idx..]
                } else {
                    ""
                };
                output.push_str(&format!("{}{}: {}{}\n", indent, key, new_value, if comment.is_empty() { String::new() } else { format!("  {}", comment) }));
                continue;
            }
        }
        output.push_str(line);
        output.push('\n');
    }
    output
}

fn parse_yaml_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.starts_with('#') || line.is_empty() {
        return None;
    }
    let parts: Vec<&str> = line.splitn(2, ':').collect();
    if parts.len() == 2 {
        let key = parts[0].trim().to_string();
        let val_part = parts[1];
        let val = val_part.split('#').next()?.trim().to_string();
        if !val.is_empty() {
            return Some((key, val));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_yaml_line_parses_key_value() {
        let line = "path: \"model.gguf\"  # comment";
        let parsed = parse_yaml_line(line);
        assert_eq!(parsed, Some(("path".to_string(), "\"model.gguf\"".to_string())));
    }

    #[test]
    fn parse_yaml_line_skips_comments() {
        assert_eq!(parse_yaml_line("# hello"), None);
        assert_eq!(parse_yaml_line("   "), None);
    }
}
