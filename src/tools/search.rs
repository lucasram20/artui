//! `search` tool — search file contents using ripgrep.

use std::process::Stdio;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::process::Command;

use crate::providers::ToolSpec;

use super::{Tool, ToolContext, ToolResult};

pub struct SearchTool;

const MAX_SEARCH_OUTPUT_CHARS: usize = 30_000;

#[async_trait]
impl Tool for SearchTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "search".to_owned(),
            description:
                "Search file contents using ripgrep. Returns matches with file:line context."
                    .to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Search pattern (regex by default)"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory or file to search within (relative to workspace, default \".\")"
                    },
                    "case_sensitive": {
                        "type": "boolean",
                        "default": false,
                        "description": "Whether the search is case-sensitive"
                    },
                    "file_glob": {
                        "type": "string",
                        "description": "Only search files matching this glob (e.g. \"*.rs\")"
                    },
                    "context_lines": {
                        "type": "integer",
                        "minimum": 0,
                        "default": 2,
                        "description": "Number of context lines around each match"
                    },
                    "max_matches": {
                        "type": "integer",
                        "minimum": 1,
                        "default": 80,
                        "description": "Maximum number of matches to return"
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

        let search_path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let case_sensitive = args
            .get("case_sensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let file_glob = args.get("file_glob").and_then(|v| v.as_str());
        let context_lines = args
            .get("context_lines")
            .and_then(|v| v.as_u64())
            .unwrap_or(2);
        let max_matches = args
            .get("max_matches")
            .and_then(|v| v.as_u64())
            .unwrap_or(80);

        // Resolve search path
        let resolved_path = ctx.workspace_root.join(search_path);
        if !resolved_path.exists() {
            return ToolResult::error(
                ctx.call_id,
                format!("search path '{}' does not exist", search_path),
            );
        }

        // Check if rg is available
        if which::which("rg").is_err() {
            return ToolResult::error(
                ctx.call_id,
                "ripgrep (rg) is not installed. Install with: dnf install ripgrep / apt install ripgrep / brew install ripgrep".to_owned(),
            );
        }

        // Build rg command
        let mut cmd = Command::new("rg");
        cmd.arg("--no-heading")
            .arg("--line-number")
            .arg("--column")
            .arg("--color=never")
            .arg(format!("--max-count={max_matches}"))
            .arg(format!("--context={context_lines}"));

        if !case_sensitive {
            cmd.arg("--ignore-case");
        }

        if let Some(fg) = file_glob {
            cmd.arg("--glob").arg(fg);
        }

        cmd.arg("--")
            .arg(pattern)
            .arg(&resolved_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = match cmd.output().await {
            Ok(o) => o,
            Err(e) => {
                return ToolResult::error(ctx.call_id, format!("failed to run rg: {e}"));
            }
        };

        // rg exits 1 when no matches found (not an error)
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() && output.status.code() != Some(1) {
            return ToolResult::error(
                ctx.call_id,
                format!("rg failed (exit {}): {}", output.status, stderr.trim()),
            );
        }

        if stdout.is_empty() {
            return ToolResult::ok(
                ctx.call_id,
                format!("No matches found for pattern '{pattern}'"),
            );
        }

        // Strip workspace prefix from output for cleaner display
        let workspace_prefix = ctx.workspace_root.to_string_lossy();
        let mut result_text = stdout.replace(&*workspace_prefix, ".");
        // Also handle trailing slash variant
        let prefix_slash = format!("{}/", workspace_prefix);
        result_text = result_text.replace(&prefix_slash, "");

        // Truncate if too large
        if result_text.len() > MAX_SEARCH_OUTPUT_CHARS {
            result_text.truncate(MAX_SEARCH_OUTPUT_CHARS);
            result_text.push_str(&format!(
                "\n\n... (truncated at {} chars)",
                MAX_SEARCH_OUTPUT_CHARS
            ));
        }

        ToolResult::ok(ctx.call_id, result_text)
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
            call_id: "s1".to_owned(),
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
    async fn search_finds_pattern() {
        if which::which("rg").is_err() {
            eprintln!("skipping: rg not installed");
            return;
        }

        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("hello.rs"),
            "fn main() {\n    println!(\"hello\");\n}\n",
        )
        .unwrap();
        fs::write(dir.path().join("other.txt"), "nothing here\n").unwrap();

        let tool = SearchTool;
        let ctx = test_ctx(dir.path());
        let result = tool
            .execute(json!({"pattern": "println", "file_glob": "*.rs"}), ctx)
            .await;

        assert!(result.error.is_none());
        assert!(result.content.contains("println"));
        assert!(result.content.contains("hello.rs"));
    }

    #[tokio::test]
    async fn search_no_matches() {
        if which::which("rg").is_err() {
            eprintln!("skipping: rg not installed");
            return;
        }

        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("file.txt"), "hello world\n").unwrap();

        let tool = SearchTool;
        let ctx = test_ctx(dir.path());
        let result = tool.execute(json!({"pattern": "zzzznotfound"}), ctx).await;

        assert!(result.error.is_none());
        assert!(result.content.contains("No matches found"));
    }

    #[tokio::test]
    async fn search_invalid_path() {
        let dir = TempDir::new().unwrap();

        let tool = SearchTool;
        let ctx = test_ctx(dir.path());
        let result = tool
            .execute(json!({"pattern": "x", "path": "nonexistent"}), ctx)
            .await;

        assert!(result.is_error());
        assert!(result.error.unwrap().contains("does not exist"));
    }
}
