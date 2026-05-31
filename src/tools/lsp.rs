//! `lsp` tool — language-server-backed code intelligence.
//!
//! Phase N1 ships three actions:
//!
//! - `definition` — resolve a symbol to its declaration site(s)
//! - `hover` — fetch type info / docs at a position
//! - `status` — report which servers are running for the workspace
//!
//! Capability gating: each action checks the server's
//! `ServerCapabilities` and returns a clear "server `X` does not advertise
//! `Y` capability" string when missing — actionable feedback the agent can
//! route around.
//!
//! Path normalization goes through `lsp_types::Url::from_file_path` end-to-end
//! to keep Windows path handling honest.
//!
//! Read-only operations bypass the approval engine — they're indistinguishable
//! from `read_file` for safety. Mutating operations land in Phase N4.

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::lsp::render;
use crate::lsp::types::LspAction;
use crate::providers::ToolSpec;

use super::{Tool, ToolContext, ToolResult};

const MAX_HOVER_CHARS: usize = 4_000;
const MAX_STATUS_CLIENTS: usize = 32;

pub struct LspTool;

#[async_trait]
impl Tool for LspTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "lsp".to_owned(),
            description: "Language-server-backed code intelligence. Actions: \
                `definition` (jump to declaration), `hover` (type info / docs), \
                `status` (which servers are running). Uses installed servers \
                like rust-analyzer, gopls, pyright, typescript-language-server, \
                clangd. Path is workspace-relative; line and column are 1-based."
                .to_owned(),
            parameters: json!({
                "type": "object",
                "required": ["action"],
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["definition", "hover", "status"],
                        "description": "Operation to perform"
                    },
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative path. Required for definition/hover."
                    },
                    "line": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "1-based line number (required for definition/hover)"
                    },
                    "column": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "1-based column number (required for definition/hover)"
                    }
                }
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: ToolContext) -> ToolResult {
        let Some(action_str) = args.get("action").and_then(|v| v.as_str()) else {
            return ToolResult::error(ctx.call_id, "missing required parameter: action".to_owned());
        };
        let Some(action) = LspAction::parse(action_str) else {
            return ToolResult::error(
                ctx.call_id,
                format!(
                    "unknown action `{action_str}` (expected one of: definition, hover, status)"
                ),
            );
        };

        let Some(manager) = ctx.lsp_manager.clone() else {
            return ToolResult::error(
                ctx.call_id,
                "language server support is disabled (set `[lsp] enabled = true` in ~/.config/artui/config.toml to enable)".to_owned(),
            );
        };

        match action {
            LspAction::Status => execute_status(manager, ctx).await,
            LspAction::Definition => execute_definition(manager, args, ctx).await,
            LspAction::Hover => execute_hover(manager, args, ctx).await,
        }
    }
}

async fn execute_status(
    manager: std::sync::Arc<crate::lsp::LspManager>,
    ctx: ToolContext,
) -> ToolResult {
    let snapshot = manager.status_snapshot().await;
    if snapshot.is_empty() {
        let mut lines = vec!["No language servers are currently running.".to_owned()];
        lines.push(format!(
            "Registered servers in registry: {} (use action `definition` or `hover` on a supported file to spawn one).",
            manager.registry().len()
        ));
        return ToolResult::ok(ctx.call_id, lines.join("\n"));
    }
    let mut out = String::new();
    out.push_str(&format!("running language servers ({})\n", snapshot.len()));
    for entry in snapshot.iter().take(MAX_STATUS_CLIENTS) {
        let state = if entry.capabilities_initialized {
            "ready"
        } else {
            "initializing"
        };
        let display_root = entry
            .root
            .strip_prefix(&ctx.workspace_root)
            .unwrap_or(&entry.root);
        out.push_str(&format!(
            "- {server_id} [{state}] {root}\n",
            server_id = entry.server_id,
            state = state,
            root = if display_root.as_os_str().is_empty() {
                ".".to_owned()
            } else {
                display_root.display().to_string()
            }
        ));
    }
    if snapshot.len() > MAX_STATUS_CLIENTS {
        out.push_str(&format!(
            "(... {} more clients omitted)\n",
            snapshot.len() - MAX_STATUS_CLIENTS
        ));
    }
    ToolResult::ok(ctx.call_id, out)
}

async fn execute_definition(
    manager: std::sync::Arc<crate::lsp::LspManager>,
    args: Value,
    ctx: ToolContext,
) -> ToolResult {
    let Some((path, line, column)) = parse_position_args(&args, &ctx) else {
        return ToolResult::error(
            ctx.call_id,
            "definition requires `path`, `line`, and `column`".to_owned(),
        );
    };

    let resolved = resolve_path(&path, &ctx.workspace_root);
    let arc_client = match manager.for_path(&resolved, &ctx.workspace_root).await {
        Ok(client) => client,
        Err(error) => return ToolResult::error(ctx.call_id, format!("{error:#}")),
    };

    let response = {
        let client = arc_client.lock().await;
        client.definition(&resolved, line, column).await
    };

    match response {
        Ok(Some(payload)) => {
            let views = render::locations_from_response(payload);
            let formatted = render::format_locations(&views, &ctx.workspace_root);
            ToolResult::ok(ctx.call_id, cap_output(formatted, ctx.max_read_file_chars))
        }
        Ok(None) => ToolResult::ok(ctx.call_id, "no definition found".to_owned()),
        Err(error) => ToolResult::error(ctx.call_id, format!("{error:#}")),
    }
}

async fn execute_hover(
    manager: std::sync::Arc<crate::lsp::LspManager>,
    args: Value,
    ctx: ToolContext,
) -> ToolResult {
    let Some((path, line, column)) = parse_position_args(&args, &ctx) else {
        return ToolResult::error(
            ctx.call_id,
            "hover requires `path`, `line`, and `column`".to_owned(),
        );
    };

    let resolved = resolve_path(&path, &ctx.workspace_root);
    let arc_client = match manager.for_path(&resolved, &ctx.workspace_root).await {
        Ok(client) => client,
        Err(error) => return ToolResult::error(ctx.call_id, format!("{error:#}")),
    };

    let response = {
        let client = arc_client.lock().await;
        client.hover(&resolved, line, column).await
    };

    match response {
        Ok(Some(hover)) => match render::hover_view(hover) {
            Some(view) => {
                let cap = ctx.max_read_file_chars.min(MAX_HOVER_CHARS);
                ToolResult::ok(ctx.call_id, cap_output(view.contents, cap))
            }
            None => ToolResult::ok(ctx.call_id, "(no hover info)".to_owned()),
        },
        Ok(None) => ToolResult::ok(ctx.call_id, "(no hover info)".to_owned()),
        Err(error) => ToolResult::error(ctx.call_id, format!("{error:#}")),
    }
}

fn parse_position_args(args: &Value, _ctx: &ToolContext) -> Option<(PathBuf, u32, u32)> {
    let path = args.get("path").and_then(|v| v.as_str())?;
    let line = args
        .get("line")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)?;
    let column = args
        .get("column")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)?;
    if line == 0 || column == 0 {
        return None;
    }
    Some((PathBuf::from(path), line, column))
}

fn resolve_path(path: &std::path::Path, workspace_root: &std::path::Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    }
}

fn cap_output(mut s: String, max_chars: usize) -> String {
    if max_chars == 0 || s.len() <= max_chars {
        return s;
    }
    s.truncate(max_chars);
    s.push_str("\n\n[output truncated]");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::{LspManager, ServerRegistry};
    use std::sync::Arc;
    use tokio::sync::mpsc;

    fn test_ctx_with_manager(
        workspace: &std::path::Path,
        manager: Option<Arc<LspManager>>,
    ) -> ToolContext {
        let (tx, _rx) = mpsc::channel(8);
        ToolContext {
            call_id: "lsp-test".to_owned(),
            workspace_root: workspace.to_path_buf(),
            cwd: workspace.to_path_buf(),
            events: tx,
            max_read_file_chars: 10_000,
            lsp_manager: manager,
        }
    }

    fn build_manager() -> (Arc<LspManager>, mpsc::Receiver<crate::app::AppEvent>) {
        let registry = ServerRegistry::from_toml_str(
            r#"
[server.rust-analyzer]
command = "rust-analyzer-zzz-not-found"
file_types = ["rs"]
root_markers = ["Cargo.toml"]
"#,
        )
        .unwrap();
        let (tx, rx) = mpsc::channel(8);
        (Arc::new(LspManager::new(registry, tx)), rx)
    }

    #[tokio::test]
    async fn errors_when_manager_disabled() {
        let dir = tempfile::TempDir::new().unwrap();
        let ctx = test_ctx_with_manager(dir.path(), None);
        let result = LspTool.execute(json!({"action": "status"}), ctx).await;
        assert!(result.is_error());
        assert!(result.error.unwrap().contains("disabled"));
    }

    #[tokio::test]
    async fn rejects_unknown_action() {
        let dir = tempfile::TempDir::new().unwrap();
        let (manager, _rx) = build_manager();
        let ctx = test_ctx_with_manager(dir.path(), Some(manager));
        let result = LspTool.execute(json!({"action": "rename"}), ctx).await;
        assert!(result.is_error());
        let msg = result.error.unwrap();
        assert!(msg.contains("unknown action"), "got: {msg}");
    }

    #[tokio::test]
    async fn status_action_lists_no_running_servers_initially() {
        let dir = tempfile::TempDir::new().unwrap();
        let (manager, _rx) = build_manager();
        let ctx = test_ctx_with_manager(dir.path(), Some(manager));
        let result = LspTool.execute(json!({"action": "status"}), ctx).await;
        assert!(!result.is_error());
        assert!(result.content.contains("No language servers"));
    }

    #[tokio::test]
    async fn definition_requires_path_line_column() {
        let dir = tempfile::TempDir::new().unwrap();
        let (manager, _rx) = build_manager();
        let ctx = test_ctx_with_manager(dir.path(), Some(manager));
        let result = LspTool.execute(json!({"action": "definition"}), ctx).await;
        assert!(result.is_error());
        let msg = result.error.unwrap();
        assert!(
            msg.contains("path") && msg.contains("line") && msg.contains("column"),
            "got: {msg}"
        );
    }

    #[tokio::test]
    async fn definition_returns_clean_error_when_server_missing() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        let (manager, _rx) = build_manager();
        let ctx = test_ctx_with_manager(dir.path(), Some(manager));
        let result = LspTool
            .execute(
                json!({"action": "definition", "path": "main.rs", "line": 1, "column": 4}),
                ctx,
            )
            .await;
        assert!(result.is_error());
        let msg = result.error.unwrap();
        assert!(msg.contains("not found"), "got: {msg}");
    }

    #[tokio::test]
    async fn definition_returns_clear_error_for_unsupported_extension() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("note.zig"), "// zig").unwrap();
        let (manager, _rx) = build_manager();
        let ctx = test_ctx_with_manager(dir.path(), Some(manager));
        let result = LspTool
            .execute(
                json!({"action": "definition", "path": "note.zig", "line": 1, "column": 1}),
                ctx,
            )
            .await;
        assert!(result.is_error());
        let msg = result.error.unwrap();
        assert!(msg.contains("no language server"), "got: {msg}");
    }

    #[test]
    fn cap_output_truncates_long_strings() {
        let big = "a".repeat(5_000);
        let out = cap_output(big, 100);
        assert!(out.starts_with("aaaa"));
        assert!(out.ends_with("[output truncated]"));
        assert!(out.len() < 200);
    }

    #[test]
    fn cap_output_passthrough_short() {
        let s = "short".to_owned();
        let out = cap_output(s.clone(), 100);
        assert_eq!(out, s);
    }
}
