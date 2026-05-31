//! Tool trait, context, result types, and registry.

pub mod apply_patch;
pub mod glob;
pub mod lsp;
pub mod read_file;
pub mod registry;
pub mod search;
pub mod shell;
pub mod task;
pub mod todo_write;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::app::AppEvent;
use crate::providers::ToolSpec;

/// Context passed to every tool execution.
#[derive(Clone)]
pub struct ToolContext {
    pub call_id: String,
    pub workspace_root: PathBuf,
    pub cwd: PathBuf,
    pub events: mpsc::Sender<AppEvent>,
    pub max_read_file_chars: usize,
    /// Optional handle to the workspace's `LspManager`. Populated when
    /// `[lsp] enabled = true`. Tools that don't need LSP ignore this; the
    /// `lsp` tool requires it and is only registered when the manager
    /// exists.
    pub lsp_manager: Option<Arc<crate::lsp::LspManager>>,
    /// Phase N3 — when true, `apply_patch` runs a writethrough pass after
    /// every successful patch. Mirrors `[lsp] writethrough` in the global
    /// config.
    pub lsp_writethrough: bool,
    /// Phase N3 — wall-clock budget for the post-apply_patch diagnostics
    /// poll. Mirrors `[lsp] diagnostics_timeout_ms`.
    pub lsp_diagnostics_timeout_ms: u32,
    // pub permissions: Arc<PermissionEngine>,  // Phase D
    // pub cancel: CancellationToken,           // Phase C
}

/// Result of a tool execution.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub call_id: String,
    pub content: String,
    pub error: Option<String>,
    pub artifact_path: Option<PathBuf>,
}

impl ToolResult {
    pub fn ok(call_id: String, content: String) -> Self {
        Self {
            call_id,
            content,
            error: None,
            artifact_path: None,
        }
    }

    pub fn error(call_id: String, error: String) -> Self {
        Self {
            call_id,
            content: String::new(),
            error: Some(error),
            artifact_path: None,
        }
    }

    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }
}

/// Trait that all tools implement.
#[async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    async fn execute(&self, args: Value, ctx: ToolContext) -> ToolResult;
}
