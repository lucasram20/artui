//! `glob` tool — list files matching a glob pattern within the workspace.

use async_trait::async_trait;
use ignore::WalkBuilder;
use serde_json::{json, Value};

use crate::providers::ToolSpec;

use super::{Tool, ToolContext, ToolResult};

pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "glob".to_owned(),
            description:
                "List files matching a glob pattern within the workspace. Respects .gitignore."
                    .to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern to match (e.g. \"src/**/*.rs\", \"*.toml\")"
                    },
                    "max_results": {
                        "type": "integer",
                        "minimum": 1,
                        "default": 200,
                        "description": "Maximum number of results to return"
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: ToolContext) -> ToolResult {
        let Some(pattern) = args.get("pattern").and_then(|v| v.as_str()) else {
            return ToolResult::error(
                ctx.call_id,
                "missing required parameter: pattern".to_owned(),
            );
        };

        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(200) as usize;

        let glob_matcher = match glob::Pattern::new(pattern) {
            Ok(m) => m,
            Err(e) => {
                return ToolResult::error(
                    ctx.call_id,
                    format!("invalid glob pattern '{}': {e}", pattern),
                );
            }
        };

        let mut results = Vec::new();
        let walker = WalkBuilder::new(&ctx.workspace_root)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .build();

        for entry in walker.flatten() {
            if results.len() >= max_results {
                break;
            }
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            // Get relative path
            let rel = match path.strip_prefix(&ctx.workspace_root) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let rel_str = rel.to_string_lossy();
            if glob_matcher.matches(&rel_str) || glob_matcher.matches_path(rel) {
                results.push(rel_str.to_string());
            }
        }

        results.sort();

        if results.is_empty() {
            return ToolResult::ok(ctx.call_id, format!("No files matched pattern '{pattern}'"));
        }

        let truncated = results.len() >= max_results;
        let mut output = results.join("\n");
        if truncated {
            output.push_str(&format!("\n\n... (limited to {max_results} results)"));
        }
        output.insert_str(0, &format!("{} files matched:\n", results.len()));

        ToolResult::ok(ctx.call_id, output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    fn test_ctx(workspace: &Path) -> ToolContext {
        let (tx, _rx) = mpsc::channel(1);
        ToolContext {
            call_id: "g1".to_owned(),
            workspace_root: workspace.to_path_buf(),
            cwd: workspace.to_path_buf(),
            events: tx,
            max_read_file_chars: 10000,
        }
    }

    #[tokio::test]
    async fn matches_rs_files() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("src/lib.rs"), "").unwrap();
        fs::write(dir.path().join("README.md"), "# hi").unwrap();

        let tool = GlobTool;
        let ctx = test_ctx(dir.path());
        let result = tool.execute(json!({"pattern": "**/*.rs"}), ctx).await;

        assert!(result.error.is_none());
        // Normalize path separators for cross-platform (Windows uses \)
        let content = result.content.replace('\\', "/");
        assert!(content.contains("src/main.rs"));
        assert!(content.contains("src/lib.rs"));
        assert!(!content.contains("README.md"));
    }

    #[tokio::test]
    async fn respects_max_results() {
        let dir = TempDir::new().unwrap();
        for i in 0..10 {
            fs::write(dir.path().join(format!("file{i}.txt")), "x").unwrap();
        }

        let tool = GlobTool;
        let ctx = test_ctx(dir.path());
        let result = tool
            .execute(json!({"pattern": "*.txt", "max_results": 3}), ctx)
            .await;

        assert!(result.error.is_none());
        assert!(result.content.contains("limited to 3 results"));
    }

    #[tokio::test]
    async fn no_matches() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("hello.txt"), "x").unwrap();

        let tool = GlobTool;
        let ctx = test_ctx(dir.path());
        let result = tool.execute(json!({"pattern": "*.xyz"}), ctx).await;

        assert!(result.error.is_none());
        assert!(result.content.contains("No files matched"));
    }
}
