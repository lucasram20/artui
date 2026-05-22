//! Model Context Protocol (MCP) client — stdio transport, v1.
//!
//! Implements just enough of MCP 2024-11-05 to discover and dispatch tools
//! from external servers configured in `.artui/mcp.json` (project) or
//! `~/.config/artui/mcp.json` (user). SSE/HTTP transport is deferred.
//!
//! Wire format: newline-delimited JSON-RPC 2.0 on the server's stdin/stdout.
//! Reference: <https://modelcontextprotocol.io/specification/2024-11-05>.
//!
//! Lifecycle:
//! 1. `McpClient::spawn` launches the server process.
//! 2. `initialize` handshake (`initialize` → `notifications/initialized`).
//! 3. `tools/list` discovery.
//! 4. Each tool registered into `ToolRegistry` as a `McpTool` adapter.
//! 5. On `tools/call`, the adapter forwards args via the same stdio channel.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{oneshot, Mutex};
use tokio::time::timeout;

use crate::providers::ToolSpec;
use crate::tools::{Tool, ToolContext, ToolResult};

/// Default request timeout for MCP RPCs.
const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(30);

// ── Configuration ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct McpConfig {
    /// Map of server-id → server definition.
    pub servers: HashMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct McpServerConfig {
    /// stdio command to spawn (e.g. ["npx", "-y", "@modelcontextprotocol/server-fs"]).
    pub command: Vec<String>,
    /// Optional working directory.
    pub cwd: Option<PathBuf>,
    /// Environment variables to inject.
    pub env: HashMap<String, String>,
    /// When false, server is registered in config but not started.
    pub enabled: bool,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            command: Vec::new(),
            cwd: None,
            env: HashMap::new(),
            enabled: true,
        }
    }
}

/// Discover MCP config from `.artui/mcp.json` (project) and merge with
/// `~/.config/artui/mcp.json` (user). Project overrides user on key collision.
pub fn load_mcp_config(workspace_root: &Path) -> McpConfig {
    let mut merged = McpConfig::default();
    if let Some(user_path) = user_config_path() {
        if let Some(cfg) = read_config_file(&user_path) {
            for (k, v) in cfg.servers {
                merged.servers.insert(k, v);
            }
        }
    }
    let project_path = workspace_root.join(".artui").join("mcp.json");
    if let Some(cfg) = read_config_file(&project_path) {
        for (k, v) in cfg.servers {
            merged.servers.insert(k, v);
        }
    }
    merged
}

fn user_config_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("artui")
            .join("mcp.json"),
    )
}

fn read_config_file(path: &Path) -> Option<McpConfig> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

// ── JSON-RPC plumbing ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct JsonRpcRequest<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcNotification<'a> {
    jsonrpc: &'static str,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    #[serde(default)]
    code: i64,
    message: String,
}

// ── Client ─────────────────────────────────────────────────────────────

/// Stateful MCP client over stdio. One client = one server process.
pub struct McpClient {
    server_id: String,
    inner: Arc<Mutex<ClientInner>>,
}

struct ClientInner {
    next_id: u64,
    pending: HashMap<u64, oneshot::Sender<Result<Value, String>>>,
    stdin: ChildStdin,
    /// Kept alive for the lifetime of the client.
    _child: Child,
}

impl McpClient {
    pub fn server_id(&self) -> &str {
        &self.server_id
    }

    /// Spawn the server, perform `initialize` handshake.
    pub async fn spawn(server_id: &str, cfg: &McpServerConfig) -> Result<Self> {
        if cfg.command.is_empty() {
            bail!("MCP server '{server_id}' has empty command");
        }
        let (program, args) = cfg.command.split_first().unwrap();
        let mut command = Command::new(program);
        command
            .args(args)
            .envs(&cfg.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if let Some(cwd) = &cfg.cwd {
            command.current_dir(cwd);
        }
        let mut child = command
            .spawn()
            .with_context(|| format!("failed to spawn MCP server '{server_id}'"))?;
        let stdin = child.stdin.take().context("MCP child stdin")?;
        let stdout = child.stdout.take().context("MCP child stdout")?;

        let inner = Arc::new(Mutex::new(ClientInner {
            next_id: 1,
            pending: HashMap::new(),
            stdin,
            _child: child,
        }));
        spawn_reader(stdout, Arc::clone(&inner));

        let client = Self {
            server_id: server_id.to_owned(),
            inner,
        };
        client.handshake().await?;
        Ok(client)
    }

    async fn handshake(&self) -> Result<()> {
        let _result = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "clientInfo": { "name": "artui", "version": env!("CARGO_PKG_VERSION") },
                }),
            )
            .await?;
        self.notify("notifications/initialized", json!({})).await?;
        Ok(())
    }

    /// `tools/list` — returns specs for every tool the server exposes.
    pub async fn list_tools(&self) -> Result<Vec<ToolSpec>> {
        let result = self.request("tools/list", json!({})).await?;
        let tools = result
            .get("tools")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut out = Vec::with_capacity(tools.len());
        for tool in tools {
            let name = tool
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned())
                .ok_or_else(|| anyhow!("tools/list entry missing 'name'"))?;
            let description = tool
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let parameters = tool
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
            out.push(ToolSpec {
                name,
                description,
                parameters,
            });
        }
        Ok(out)
    }

    /// `tools/call` — invoke a tool and return its content as a string.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<String> {
        let result = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await?;
        // MCP returns content: [{type: "text", text: "..."}]
        let content = result.get("content").and_then(|v| v.as_array());
        let mut buf = String::new();
        if let Some(items) = content {
            for item in items {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    if !buf.is_empty() {
                        buf.push('\n');
                    }
                    buf.push_str(text);
                }
            }
        }
        if buf.is_empty() {
            // Fall back to the raw result so the model still sees something.
            buf = result.to_string();
        }
        let is_error = result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if is_error {
            bail!(buf);
        }
        Ok(buf)
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let (tx, rx) = oneshot::channel();
        let id = {
            let mut guard = self.inner.lock().await;
            let id = guard.next_id;
            guard.next_id = id.wrapping_add(1);
            guard.pending.insert(id, tx);
            let req = JsonRpcRequest {
                jsonrpc: "2.0",
                id,
                method,
                params,
            };
            let line = serde_json::to_string(&req)?;
            guard.stdin.write_all(line.as_bytes()).await?;
            guard.stdin.write_all(b"\n").await?;
            guard.stdin.flush().await?;
            id
        };
        match timeout(DEFAULT_RPC_TIMEOUT, rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(message))) => bail!("MCP error: {message}"),
            Ok(Err(_)) => bail!("MCP channel closed"),
            Err(_) => {
                // Reclaim slot so it doesn't leak if the server never replies.
                let mut guard = self.inner.lock().await;
                guard.pending.remove(&id);
                bail!("MCP request '{method}' timed out");
            }
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        let mut guard = self.inner.lock().await;
        let note = JsonRpcNotification {
            jsonrpc: "2.0",
            method,
            params,
        };
        let line = serde_json::to_string(&note)?;
        guard.stdin.write_all(line.as_bytes()).await?;
        guard.stdin.write_all(b"\n").await?;
        guard.stdin.flush().await?;
        Ok(())
    }
}

fn spawn_reader(stdout: ChildStdout, inner: Arc<Mutex<ClientInner>>) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let response: JsonRpcResponse = match serde_json::from_str(line) {
                Ok(value) => value,
                Err(_) => continue, // ignore malformed lines / notifications
            };
            let Some(id) = response.id else {
                continue; // server-originated notification — not handled v1
            };
            let mut guard = inner.lock().await;
            if let Some(tx) = guard.pending.remove(&id) {
                let payload = if let Some(error) = response.error {
                    Err(format!("[{}] {}", error.code, error.message))
                } else {
                    Ok(response.result.unwrap_or(Value::Null))
                };
                let _ = tx.send(payload);
            }
        }
    });
}

// ── Tool adapter ───────────────────────────────────────────────────────

/// Adapter that exposes a remote MCP tool through the local `Tool` trait.
pub struct McpTool {
    spec: ToolSpec,
    remote_name: String,
    client: Arc<McpClient>,
    server_id: String,
}

impl McpTool {
    pub fn new(
        spec: ToolSpec,
        remote_name: String,
        client: Arc<McpClient>,
        server_id: String,
    ) -> Self {
        Self {
            spec,
            remote_name,
            client,
            server_id,
        }
    }
}

#[async_trait]
impl Tool for McpTool {
    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    async fn execute(&self, args: Value, ctx: ToolContext) -> ToolResult {
        match self.client.call_tool(&self.remote_name, args).await {
            Ok(content) => ToolResult::ok(ctx.call_id, content),
            Err(error) => {
                ToolResult::error(ctx.call_id, format!("[mcp:{}] {error}", self.server_id))
            }
        }
    }
}

// ── Manager ────────────────────────────────────────────────────────────

/// Spawns every enabled server in `cfg`, registers each tool into `registry`.
/// Tool names are namespaced as `<server_id>__<remote_tool_name>` to avoid
/// collisions with built-in tools.
pub async fn register_servers(
    cfg: &McpConfig,
    registry: &mut crate::tools::registry::ToolRegistry,
) -> Vec<ServerStatus> {
    let mut statuses = Vec::with_capacity(cfg.servers.len());
    for (server_id, server_cfg) in &cfg.servers {
        if !server_cfg.enabled {
            statuses.push(ServerStatus {
                server_id: server_id.clone(),
                state: ServerState::Disabled,
                tool_count: 0,
                error: None,
            });
            continue;
        }
        match spawn_and_register_one(server_id, server_cfg, registry).await {
            Ok(count) => statuses.push(ServerStatus {
                server_id: server_id.clone(),
                state: ServerState::Connected,
                tool_count: count,
                error: None,
            }),
            Err(error) => statuses.push(ServerStatus {
                server_id: server_id.clone(),
                state: ServerState::Failed,
                tool_count: 0,
                error: Some(error.to_string()),
            }),
        }
    }
    statuses
}

async fn spawn_and_register_one(
    server_id: &str,
    server_cfg: &McpServerConfig,
    registry: &mut crate::tools::registry::ToolRegistry,
) -> Result<usize> {
    let client = Arc::new(McpClient::spawn(server_id, server_cfg).await?);
    let specs = client.list_tools().await?;
    let count = specs.len();
    for mut spec in specs {
        let remote_name = spec.name.clone();
        // Namespace to prevent collisions with built-in tools.
        let local_name = format!("{server_id}__{}", spec.name);
        spec.name = local_name.clone();
        let tool = Arc::new(McpTool::new(
            spec,
            remote_name,
            Arc::clone(&client),
            server_id.to_owned(),
        ));
        registry.register(tool);
    }
    Ok(count)
}

#[derive(Debug, Clone)]
pub struct ServerStatus {
    pub server_id: String,
    pub state: ServerState,
    pub tool_count: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerState {
    Connected,
    Disabled,
    Failed,
}

impl ServerState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Disabled => "disabled",
            Self::Failed => "failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_mcp_config() {
        let raw = r#"{
            "servers": {
                "fs": {
                    "command": ["npx", "-y", "@modelcontextprotocol/server-fs"],
                    "enabled": true
                }
            }
        }"#;
        let cfg: McpConfig = serde_json::from_str(raw).unwrap();
        assert_eq!(cfg.servers.len(), 1);
        let server = cfg.servers.get("fs").unwrap();
        assert_eq!(
            server.command,
            vec!["npx", "-y", "@modelcontextprotocol/server-fs"]
        );
        assert!(server.enabled);
    }

    #[test]
    fn defaults_enabled_to_true() {
        let raw = r#"{ "servers": { "x": { "command": ["echo"] } } }"#;
        let cfg: McpConfig = serde_json::from_str(raw).unwrap();
        assert!(cfg.servers.get("x").unwrap().enabled);
    }

    #[test]
    fn empty_config_when_no_files() {
        // Arrange: workspace dir with no .artui/mcp.json, and HOME pointing
        // at a tmp dir without the user config either.
        let tmp = tempfile::TempDir::new().unwrap();
        let prev_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", tmp.path());
        let cfg = load_mcp_config(tmp.path());
        match prev_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        assert!(cfg.servers.is_empty());
    }

    #[test]
    fn server_state_labels() {
        assert_eq!(ServerState::Connected.label(), "connected");
        assert_eq!(ServerState::Disabled.label(), "disabled");
        assert_eq!(ServerState::Failed.label(), "failed");
    }
}
