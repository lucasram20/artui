//! `lsp` tool — language-server-backed code intelligence.
//!
//! Phase N1 ships:
//!   - `definition` — resolve a symbol to its declaration site(s)
//!   - `hover`      — fetch type info / docs at a position
//!   - `status`     — report which servers are running
//!
//! Phase N2 adds the rest of the read-only surface:
//!   - `references`        — find every caller / usage of the symbol
//!   - `implementation`    — find concrete impls of a trait/iface
//!   - `type_definition`   — find the type of a value
//!   - `document_symbols`  — list symbols in a file (tree-rendered)
//!   - `workspace_symbols` — search symbols across the workspace by name
//!   - `diagnostics`       — read the cached `publishDiagnostics` for a path
//!
//! Phase N4 adds mutating ops, gated behind the approval engine:
//!   - `rename`       — `textDocument/rename` → `WorkspaceEdit` → approval
//!   - `code_actions` — list available actions; `apply` arg invokes one
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
//! from `read_file` for safety. Mutating operations use the same approval
//! pipeline as `apply_patch`.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::lsp::edits;
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
                `definition`, `hover`, `references`, `implementation`, `type_definition`, \
                `document_symbols`, `workspace_symbols`, `diagnostics`, `rename`, \
                `code_actions`, `status`. Uses installed servers like rust-analyzer, \
                gopls, pyright, typescript-language-server, clangd. Path is \
                workspace-relative; line and column are 1-based. `rename` and \
                `code_actions` (with `apply`) route through the approval engine."
                .to_owned(),
            parameters: json!({
                "type": "object",
                "required": ["action"],
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "definition", "hover", "status",
                            "references", "implementation", "type_definition",
                            "document_symbols", "workspace_symbols", "diagnostics",
                            "rename", "code_actions"
                        ],
                        "description": "Operation to perform"
                    },
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative path. Required for definition / hover / references / implementation / type_definition / document_symbols / diagnostics / rename / code_actions."
                    },
                    "line": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "1-based line number (required for position-based actions)"
                    },
                    "column": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "1-based column number (required for position-based actions)"
                    },
                    "query": {
                        "type": "string",
                        "description": "Search string for `workspace_symbols`."
                    },
                    "new_name": {
                        "type": "string",
                        "description": "New identifier for `rename`."
                    },
                    "include_declaration": {
                        "type": "boolean",
                        "description": "For `references`: include the declaration in the result list (default true)."
                    },
                    "apply": {
                        "type": "string",
                        "description": "For `code_actions`: title of the action to apply. Listing variant returns the menu when omitted."
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
                    "unknown action `{action_str}` (expected one of: definition, hover, status, \
                     references, implementation, type_definition, document_symbols, \
                     workspace_symbols, diagnostics, rename, code_actions)"
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
            LspAction::References => execute_references(manager, args, ctx).await,
            LspAction::Implementation => execute_implementation(manager, args, ctx).await,
            LspAction::TypeDefinition => execute_type_definition(manager, args, ctx).await,
            LspAction::DocumentSymbols => execute_document_symbols(manager, args, ctx).await,
            LspAction::WorkspaceSymbols => execute_workspace_symbols(manager, args, ctx).await,
            LspAction::Diagnostics => execute_diagnostics(manager, args, ctx).await,
            LspAction::Rename => execute_rename(manager, args, ctx).await,
            LspAction::CodeActions => execute_code_actions(manager, args, ctx).await,
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
    let Some((path, line, column)) = parse_position_args(&args) else {
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
    let Some((path, line, column)) = parse_position_args(&args) else {
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

async fn execute_references(
    manager: std::sync::Arc<crate::lsp::LspManager>,
    args: Value,
    ctx: ToolContext,
) -> ToolResult {
    let Some((path, line, column)) = parse_position_args(&args) else {
        return ToolResult::error(
            ctx.call_id,
            "references requires `path`, `line`, and `column`".to_owned(),
        );
    };
    let include_declaration = args
        .get("include_declaration")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let resolved = resolve_path(&path, &ctx.workspace_root);
    let arc_client = match manager.for_path(&resolved, &ctx.workspace_root).await {
        Ok(client) => client,
        Err(error) => return ToolResult::error(ctx.call_id, format!("{error:#}")),
    };

    let response = {
        let client = arc_client.lock().await;
        client
            .references(&resolved, line, column, include_declaration)
            .await
    };

    match response {
        Ok(locations) => {
            let total = locations.len();
            let views = render::references_from_locations(locations, read_line_at);
            let formatted = render::format_references(&views, &ctx.workspace_root, total);
            ToolResult::ok(ctx.call_id, cap_output(formatted, ctx.max_read_file_chars))
        }
        Err(error) => ToolResult::error(ctx.call_id, format!("{error:#}")),
    }
}

async fn execute_implementation(
    manager: std::sync::Arc<crate::lsp::LspManager>,
    args: Value,
    ctx: ToolContext,
) -> ToolResult {
    let Some((path, line, column)) = parse_position_args(&args) else {
        return ToolResult::error(
            ctx.call_id,
            "implementation requires `path`, `line`, and `column`".to_owned(),
        );
    };
    let resolved = resolve_path(&path, &ctx.workspace_root);
    let arc_client = match manager.for_path(&resolved, &ctx.workspace_root).await {
        Ok(client) => client,
        Err(error) => return ToolResult::error(ctx.call_id, format!("{error:#}")),
    };
    let response = {
        let client = arc_client.lock().await;
        client.implementation(&resolved, line, column).await
    };
    match response {
        Ok(Some(payload)) => {
            let views = render::locations_from_response(payload);
            let formatted = if views.is_empty() {
                "no implementations found".to_owned()
            } else {
                render::format_locations(&views, &ctx.workspace_root)
            };
            ToolResult::ok(ctx.call_id, cap_output(formatted, ctx.max_read_file_chars))
        }
        Ok(None) => ToolResult::ok(ctx.call_id, "no implementations found".to_owned()),
        Err(error) => ToolResult::error(ctx.call_id, format!("{error:#}")),
    }
}

async fn execute_type_definition(
    manager: std::sync::Arc<crate::lsp::LspManager>,
    args: Value,
    ctx: ToolContext,
) -> ToolResult {
    let Some((path, line, column)) = parse_position_args(&args) else {
        return ToolResult::error(
            ctx.call_id,
            "type_definition requires `path`, `line`, and `column`".to_owned(),
        );
    };
    let resolved = resolve_path(&path, &ctx.workspace_root);
    let arc_client = match manager.for_path(&resolved, &ctx.workspace_root).await {
        Ok(client) => client,
        Err(error) => return ToolResult::error(ctx.call_id, format!("{error:#}")),
    };
    let response = {
        let client = arc_client.lock().await;
        client.type_definition(&resolved, line, column).await
    };
    match response {
        Ok(Some(payload)) => {
            let views = render::locations_from_response(payload);
            let formatted = if views.is_empty() {
                "no type definition found".to_owned()
            } else {
                render::format_locations(&views, &ctx.workspace_root)
            };
            ToolResult::ok(ctx.call_id, cap_output(formatted, ctx.max_read_file_chars))
        }
        Ok(None) => ToolResult::ok(ctx.call_id, "no type definition found".to_owned()),
        Err(error) => ToolResult::error(ctx.call_id, format!("{error:#}")),
    }
}

async fn execute_document_symbols(
    manager: std::sync::Arc<crate::lsp::LspManager>,
    args: Value,
    ctx: ToolContext,
) -> ToolResult {
    let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
        return ToolResult::error(ctx.call_id, "document_symbols requires `path`".to_owned());
    };
    let resolved = resolve_path(Path::new(path), &ctx.workspace_root);
    let arc_client = match manager.for_path(&resolved, &ctx.workspace_root).await {
        Ok(client) => client,
        Err(error) => return ToolResult::error(ctx.call_id, format!("{error:#}")),
    };
    let response = {
        let client = arc_client.lock().await;
        client.document_symbols(&resolved).await
    };
    match response {
        Ok(Some(payload)) => {
            let views = render::flatten_document_symbols(payload);
            let formatted = render::format_document_symbols(&views);
            ToolResult::ok(ctx.call_id, cap_output(formatted, ctx.max_read_file_chars))
        }
        Ok(None) => ToolResult::ok(ctx.call_id, "(no symbols)".to_owned()),
        Err(error) => ToolResult::error(ctx.call_id, format!("{error:#}")),
    }
}

async fn execute_workspace_symbols(
    manager: std::sync::Arc<crate::lsp::LspManager>,
    args: Value,
    ctx: ToolContext,
) -> ToolResult {
    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");

    // workspace/symbol can run against any spawned client. If none is
    // running, ask the model to bootstrap by hitting `definition` first.
    let snapshot = manager.status_snapshot().await;
    let Some(entry) = snapshot.first() else {
        return ToolResult::ok(
            ctx.call_id,
            "no language server is running yet — call `definition` or `hover` on a workspace file first to spawn one"
                .to_owned(),
        );
    };
    let arc_client = match manager
        .get_or_spawn_existing(entry.server_id.clone(), entry.root.clone())
        .await
    {
        Some(c) => c,
        None => return ToolResult::error(ctx.call_id, "lost language server handle".to_owned()),
    };
    let response = {
        let client = arc_client.lock().await;
        client.workspace_symbols(query).await
    };
    match response {
        Ok(symbols) => {
            let formatted = render::format_workspace_symbols(&symbols, &ctx.workspace_root);
            ToolResult::ok(ctx.call_id, cap_output(formatted, ctx.max_read_file_chars))
        }
        Err(error) => ToolResult::error(ctx.call_id, format!("{error:#}")),
    }
}

async fn execute_diagnostics(
    manager: std::sync::Arc<crate::lsp::LspManager>,
    args: Value,
    ctx: ToolContext,
) -> ToolResult {
    let path_opt = args.get("path").and_then(|v| v.as_str());

    // No path → return all cached diagnostics across all running servers.
    let snapshot = manager.status_snapshot().await;
    if snapshot.is_empty() {
        return ToolResult::ok(
            ctx.call_id,
            "no language server is running yet — diagnostics cache is empty".to_owned(),
        );
    }

    let mut all_views = Vec::new();
    for entry in &snapshot {
        let Some(arc_client) = manager
            .get_or_spawn_existing(entry.server_id.clone(), entry.root.clone())
            .await
        else {
            continue;
        };
        let path_filter: Option<PathBuf> =
            path_opt.map(|p| resolve_path(Path::new(p), &ctx.workspace_root));
        let cached = {
            let client = arc_client.lock().await;
            client.cached_diagnostics(path_filter.as_deref()).await
        };
        for (path, diags) in cached {
            all_views.extend(render::diagnostic_views(&path, &diags));
        }
    }

    let formatted = render::format_diagnostics(&all_views, &ctx.workspace_root);
    ToolResult::ok(ctx.call_id, cap_output(formatted, ctx.max_read_file_chars))
}

async fn execute_rename(
    manager: std::sync::Arc<crate::lsp::LspManager>,
    args: Value,
    ctx: ToolContext,
) -> ToolResult {
    let Some((path, line, column)) = parse_position_args(&args) else {
        return ToolResult::error(
            ctx.call_id,
            "rename requires `path`, `line`, and `column`".to_owned(),
        );
    };
    let Some(new_name) = args.get("new_name").and_then(|v| v.as_str()) else {
        return ToolResult::error(ctx.call_id, "rename requires `new_name`".to_owned());
    };
    if new_name.trim().is_empty() {
        return ToolResult::error(
            ctx.call_id,
            "rename `new_name` must not be empty".to_owned(),
        );
    }

    let resolved = resolve_path(&path, &ctx.workspace_root);
    let arc_client = match manager.for_path(&resolved, &ctx.workspace_root).await {
        Ok(client) => client,
        Err(error) => return ToolResult::error(ctx.call_id, format!("{error:#}")),
    };

    // 1) prepareRename to gate non-renameable positions early.
    let prepare = {
        let client = arc_client.lock().await;
        client.prepare_rename(&resolved, line, column).await
    };
    if let Err(error) = prepare {
        return ToolResult::error(ctx.call_id, format!("{error:#}"));
    }

    // 2) rename → WorkspaceEdit
    let edit = {
        let client = arc_client.lock().await;
        client.rename(&resolved, line, column, new_name).await
    };
    let edit = match edit {
        Ok(Some(e)) => e,
        Ok(None) => {
            return ToolResult::ok(
                ctx.call_id,
                "rename produced no edits (server returned null)".to_owned(),
            )
        }
        Err(error) => return ToolResult::error(ctx.call_id, format!("{error:#}")),
    };

    // 3) Apply the WorkspaceEdit to disk through the same path apply_patch
    //    uses. The apply_patch tool is the canonical write site, so the
    //    edit fanout reuses its workspace-relative file write logic.
    let summary = render::format_workspace_edit_summary(&edit, &ctx.workspace_root);
    match edits::apply_workspace_edit(&edit, &ctx.workspace_root).await {
        Ok(report) => {
            let body = format!(
                "{summary}\n\napplied {} of {} files",
                report.applied, report.total
            );
            ToolResult::ok(ctx.call_id, cap_output(body, ctx.max_read_file_chars))
        }
        Err(error) => ToolResult::error(ctx.call_id, format!("{summary}\n\n{error:#}")),
    }
}

async fn execute_code_actions(
    manager: std::sync::Arc<crate::lsp::LspManager>,
    args: Value,
    ctx: ToolContext,
) -> ToolResult {
    let Some((path, line, column)) = parse_position_args(&args) else {
        return ToolResult::error(
            ctx.call_id,
            "code_actions requires `path`, `line`, and `column`".to_owned(),
        );
    };
    let resolved = resolve_path(&path, &ctx.workspace_root);
    let arc_client = match manager.for_path(&resolved, &ctx.workspace_root).await {
        Ok(client) => client,
        Err(error) => return ToolResult::error(ctx.call_id, format!("{error:#}")),
    };
    let response = {
        let client = arc_client.lock().await;
        client.code_actions(&resolved, line, column).await
    };
    let actions = match response {
        Ok(actions) => actions,
        Err(error) => return ToolResult::error(ctx.call_id, format!("{error:#}")),
    };

    // Listing variant: `apply` not provided.
    let apply = args.get("apply").and_then(|v| v.as_str());
    if apply.is_none() {
        if actions.is_empty() {
            return ToolResult::ok(ctx.call_id, "(no code actions available)".to_owned());
        }
        let mut out = String::new();
        for (i, a) in actions.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            let title = match a {
                lsp_types::CodeActionOrCommand::CodeAction(action) => action.title.as_str(),
                lsp_types::CodeActionOrCommand::Command(cmd) => cmd.title.as_str(),
            };
            out.push_str(&format!("[{i}] {title}"));
        }
        out.push_str("\n\n(Pass `apply: \"<title>\"` to invoke an action.)");
        return ToolResult::ok(ctx.call_id, cap_output(out, ctx.max_read_file_chars));
    }

    // Apply variant: find by title (model passes the displayed string).
    let apply_title = apply.unwrap();
    let chosen = actions.iter().find(|a| match a {
        lsp_types::CodeActionOrCommand::CodeAction(action) => action.title == apply_title,
        lsp_types::CodeActionOrCommand::Command(cmd) => cmd.title == apply_title,
    });
    let Some(chosen) = chosen else {
        return ToolResult::error(
            ctx.call_id,
            format!("no code action titled `{apply_title}` (use the listing variant first)"),
        );
    };

    match chosen {
        lsp_types::CodeActionOrCommand::CodeAction(action) => {
            if let Some(edit) = &action.edit {
                let summary = render::format_workspace_edit_summary(edit, &ctx.workspace_root);
                match edits::apply_workspace_edit(edit, &ctx.workspace_root).await {
                    Ok(report) => ToolResult::ok(
                        ctx.call_id,
                        format!(
                            "{summary}\n\napplied {} of {} files",
                            report.applied, report.total
                        ),
                    ),
                    Err(error) => ToolResult::error(ctx.call_id, format!("{summary}\n\n{error:#}")),
                }
            } else {
                ToolResult::ok(
                    ctx.call_id,
                    format!(
                        "code action `{}` produced no edits (server-side command not executed; \
                         executeCommand is out of scope for v0.7.0)",
                        action.title
                    ),
                )
            }
        }
        lsp_types::CodeActionOrCommand::Command(_) => ToolResult::ok(
            ctx.call_id,
            "server-side commands (executeCommand) are not yet supported".to_owned(),
        ),
    }
}

fn parse_position_args(args: &Value) -> Option<(PathBuf, u32, u32)> {
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

/// Read a single line from disk (1-based). Returns None on IO failure.
fn read_line_at(path: &Path, line: u32) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    contents
        .lines()
        .nth(line.saturating_sub(1) as usize)
        .map(|s| s.to_owned())
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
            lsp_writethrough: false,
            lsp_diagnostics_timeout_ms: 750,
            sandbox: crate::sandbox::SandboxSettings::default(),
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
        let result = LspTool
            .execute(json!({"action": "open_file_in_editor"}), ctx)
            .await;
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
        std::fs::write(dir.path().join("note.xyz"), "// xyz").unwrap();
        let (manager, _rx) = build_manager();
        let ctx = test_ctx_with_manager(dir.path(), Some(manager));
        let result = LspTool
            .execute(
                json!({"action": "definition", "path": "note.xyz", "line": 1, "column": 1}),
                ctx,
            )
            .await;
        assert!(result.is_error());
        let msg = result.error.unwrap();
        assert!(msg.contains("no language server"), "got: {msg}");
    }

    #[tokio::test]
    async fn references_requires_position_args() {
        let dir = tempfile::TempDir::new().unwrap();
        let (manager, _rx) = build_manager();
        let ctx = test_ctx_with_manager(dir.path(), Some(manager));
        let result = LspTool
            .execute(json!({"action": "references", "path": "main.rs"}), ctx)
            .await;
        assert!(result.is_error());
        assert!(result.error.unwrap().contains("references requires"));
    }

    #[tokio::test]
    async fn document_symbols_requires_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let (manager, _rx) = build_manager();
        let ctx = test_ctx_with_manager(dir.path(), Some(manager));
        let result = LspTool
            .execute(json!({"action": "document_symbols"}), ctx)
            .await;
        assert!(result.is_error());
        assert!(result.error.unwrap().contains("requires `path`"));
    }

    #[tokio::test]
    async fn workspace_symbols_explains_no_running_server() {
        let dir = tempfile::TempDir::new().unwrap();
        let (manager, _rx) = build_manager();
        let ctx = test_ctx_with_manager(dir.path(), Some(manager));
        let result = LspTool
            .execute(json!({"action": "workspace_symbols", "query": "Foo"}), ctx)
            .await;
        assert!(!result.is_error());
        assert!(result.content.contains("no language server is running"));
    }

    #[tokio::test]
    async fn diagnostics_explains_empty_cache() {
        let dir = tempfile::TempDir::new().unwrap();
        let (manager, _rx) = build_manager();
        let ctx = test_ctx_with_manager(dir.path(), Some(manager));
        let result = LspTool.execute(json!({"action": "diagnostics"}), ctx).await;
        assert!(!result.is_error());
        assert!(result.content.contains("no language server is running"));
    }

    #[tokio::test]
    async fn rename_requires_new_name() {
        let dir = tempfile::TempDir::new().unwrap();
        let (manager, _rx) = build_manager();
        let ctx = test_ctx_with_manager(dir.path(), Some(manager));
        let result = LspTool
            .execute(
                json!({"action": "rename", "path": "main.rs", "line": 1, "column": 4}),
                ctx,
            )
            .await;
        assert!(result.is_error());
        assert!(result.error.unwrap().contains("new_name"));
    }

    #[tokio::test]
    async fn rename_rejects_empty_new_name() {
        let dir = tempfile::TempDir::new().unwrap();
        let (manager, _rx) = build_manager();
        let ctx = test_ctx_with_manager(dir.path(), Some(manager));
        let result = LspTool
            .execute(
                json!({"action": "rename", "path": "main.rs", "line": 1, "column": 4, "new_name": "   "}),
                ctx,
            )
            .await;
        assert!(result.is_error());
        assert!(result.error.unwrap().contains("must not be empty"));
    }

    #[tokio::test]
    async fn code_actions_requires_position_args() {
        let dir = tempfile::TempDir::new().unwrap();
        let (manager, _rx) = build_manager();
        let ctx = test_ctx_with_manager(dir.path(), Some(manager));
        let result = LspTool
            .execute(json!({"action": "code_actions", "path": "main.rs"}), ctx)
            .await;
        assert!(result.is_error());
        assert!(result.error.unwrap().contains("requires"));
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
