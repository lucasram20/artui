//! Tool registry — holds all registered tools and dispatches calls.

use std::collections::HashMap;
use std::sync::Arc;

use crate::providers::{ToolCall, ToolSpec};

use super::apply_patch::ApplyPatchTool;
use super::glob::GlobTool;
use super::read_file::ReadFileTool;
use super::search::SearchTool;
use super::shell::ShellTool;
use super::{Tool, ToolContext, ToolResult};

/// Registry of available tools. Immutable after construction.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// Create a registry with the default tool set.
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
        Self { tools }
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
}
