//! Tool registry — holds all registered tools and dispatches calls.

use std::collections::HashMap;
use std::sync::Arc;

use crate::providers::{ToolCall, ToolSpec};

use super::apply_patch::ApplyPatchTool;
use super::glob::GlobTool;
use super::lsp::LspTool;
use super::read_file::ReadFileTool;
use super::search::SearchTool;
use super::shell::ShellTool;
use super::todo_write::TodoWriteTool;
use super::{Tool, ToolContext, ToolResult};

/// Registry of available tools. Immutable after construction.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// Create a registry with the full tool set (for Build agent).
    pub fn new() -> Self {
        let mut tools: HashMap<String, Arc<dyn Tool>> = HashMap::new();
        let read_file = Arc::new(ReadFileTool);
        tools.insert(read_file.spec().name.clone(), read_file);
        let glob = Arc::new(GlobTool);
        tools.insert(glob.spec().name.clone(), glob);
        let search = Arc::new(SearchTool);
        tools.insert(search.spec().name.clone(), search);
        let apply_patch = Arc::new(ApplyPatchTool);
        tools.insert(apply_patch.spec().name.clone(), apply_patch);
        let shell = Arc::new(ShellTool);
        tools.insert(shell.spec().name.clone(), shell);
        let todo_write = Arc::new(TodoWriteTool);
        tools.insert(todo_write.spec().name.clone(), todo_write);
        // Note: `task` tool is registered separately via `with_task_tool`
        // because it needs a provider reference.
        // Note: `lsp` tool is registered via `with_lsp_tool` and only when
        // an `LspManager` exists — dispatching via context-less `new()`
        // would surface a "language server support is disabled" error to
        // the model on every call. Wiring here would also force every test
        // to plumb a manager.
        Self { tools }
    }

    /// Add the `lsp` tool. Called from `app.rs` after constructing
    /// the [`crate::lsp::LspManager`]. No-op when LSP is disabled.
    pub fn with_lsp_tool(mut self) -> Self {
        let lsp = Arc::new(LspTool);
        self.tools.insert(lsp.spec().name.clone(), lsp);
        self
    }

    /// Create a registry for subagents. No `task` tool (prevents recursion).
    /// If `read_only`, only includes read_file, glob, search.
    pub fn for_subagent(read_only: bool) -> Self {
        let mut tools: HashMap<String, Arc<dyn Tool>> = HashMap::new();
        let read_file = Arc::new(ReadFileTool);
        tools.insert(read_file.spec().name.clone(), read_file);
        let glob = Arc::new(GlobTool);
        tools.insert(glob.spec().name.clone(), glob);
        let search = Arc::new(SearchTool);
        tools.insert(search.spec().name.clone(), search);

        if !read_only {
            let apply_patch = Arc::new(ApplyPatchTool);
            tools.insert(apply_patch.spec().name.clone(), apply_patch);
            let shell = Arc::new(ShellTool);
            tools.insert(shell.spec().name.clone(), shell);
        }

        Self { tools }
    }

    /// Add a tool to the registry (used for task tool which needs provider ref).
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.spec().name.clone(), tool);
    }

    /// Return specs for all registered tools (for populating `ModelRequest.tools`).
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|tool| tool.spec()).collect()
    }

    /// Dispatch a tool call to the appropriate handler.
    pub async fn dispatch(&self, call: &ToolCall, mut ctx: ToolContext) -> ToolResult {
        let Some(tool) = self.tools.get(&call.name) else {
            return ToolResult::error(call.id.clone(), format!("unknown tool: '{}'", call.name));
        };
        ctx.call_id = call.id.clone();
        tool.execute(call.arguments.clone(), ctx).await
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    fn test_ctx(workspace: &Path) -> ToolContext {
        let (tx, _rx) = mpsc::channel(1);
        ToolContext {
            call_id: "test-call".to_owned(),
            workspace_root: workspace.to_path_buf(),
            cwd: workspace.to_path_buf(),
            events: tx,
            max_read_file_chars: 10000,
            lsp_manager: None,
            lsp_writethrough: false,
            lsp_diagnostics_timeout_ms: 750,
            sandbox: crate::sandbox::SandboxSettings::default(),
        }
    }

    #[tokio::test]
    async fn dispatch_read_file() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("test.txt"), "hello world\n").unwrap();

        let registry = ToolRegistry::new();
        let call = ToolCall {
            id: "call-1".to_owned(),
            name: "read_file".to_owned(),
            arguments: json!({"path": "test.txt"}),
        };
        let ctx = test_ctx(dir.path());
        let result = registry.dispatch(&call, ctx).await;

        assert!(result.error.is_none());
        assert!(result.content.contains("hello world"));
        assert_eq!(result.call_id, "call-1");
    }

    #[tokio::test]
    async fn dispatch_unknown_tool() {
        let dir = TempDir::new().unwrap();
        let registry = ToolRegistry::new();
        let call = ToolCall {
            id: "call-2".to_owned(),
            name: "nonexistent".to_owned(),
            arguments: json!({}),
        };
        let ctx = test_ctx(dir.path());
        let result = registry.dispatch(&call, ctx).await;

        assert!(result.is_error());
        assert!(result.error.unwrap().contains("unknown tool"));
    }

    #[test]
    fn specs_includes_read_file() {
        let registry = ToolRegistry::new();
        let specs = registry.specs();
        assert!(specs.iter().any(|s| s.name == "read_file"));
    }

    #[test]
    fn subagent_read_only_has_three_tools() {
        let registry = ToolRegistry::for_subagent(true);
        let specs = registry.specs();
        assert_eq!(specs.len(), 3);
        assert!(specs
            .iter()
            .all(|s| ["read_file", "glob", "search"].contains(&s.name.as_str())));
    }

    #[test]
    fn subagent_general_has_no_task() {
        let registry = ToolRegistry::for_subagent(false);
        let specs = registry.specs();
        assert!(specs.iter().any(|s| s.name == "apply_patch"));
        assert!(!specs.iter().any(|s| s.name == "task"));
    }
}
