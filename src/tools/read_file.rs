//! `read_file` tool — reads a file from the workspace with line numbers.

use std::path::Path;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::providers::ToolSpec;

use super::{Tool, ToolContext, ToolResult};

pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".to_owned(),
            description: "Read a file from the workspace with optional line range. Output includes line numbers.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path relative to workspace root"
                    },
                    "start_line": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "First line to read (1-indexed, inclusive)"
                    },
                    "end_line": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Last line to read (1-indexed, inclusive)"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: ToolContext) -> ToolResult {
        let Some(path_str) = args.get("path").and_then(|v| v.as_str()) else {
            return ToolResult::error(ctx.call_id, "missing required parameter: path".to_owned());
        };

        let start_line = args
            .get("start_line")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);
        let end_line = args
            .get("end_line")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        // Resolve and validate path
        let requested = Path::new(path_str);
        let resolved = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            ctx.workspace_root.join(requested)
        };

        let canonical = match resolved.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                return ToolResult::error(
                    ctx.call_id,
                    format!("cannot resolve path '{}': {e}", path_str),
                );
            }
        };

        // Reject path traversal outside workspace
        let canonical_workspace = ctx
            .workspace_root
            .canonicalize()
            .unwrap_or(ctx.workspace_root.clone());
        if !canonical.starts_with(&canonical_workspace) {
            return ToolResult::error(
                ctx.call_id,
                format!("path '{}' is outside the workspace", path_str),
            );
        }

        // Read file
        let content = match std::fs::read(&canonical) {
            Ok(bytes) => bytes,
            Err(e) => {
                return ToolResult::error(ctx.call_id, format!("cannot read '{}': {e}", path_str));
            }
        };

        // Binary detection: check first 8KB for null bytes
        let check_len = content.len().min(8192);
        if content[..check_len].contains(&0) {
            return ToolResult::error(
                ctx.call_id,
                format!("'{}' appears to be a binary file", path_str),
            );
        }

        let text = String::from_utf8_lossy(&content);
        let lines: Vec<&str> = text.lines().collect();
        let total_lines = lines.len();

        // Apply line range (1-indexed)
        let start = start_line.unwrap_or(1).max(1).min(total_lines + 1);
        let end = end_line.unwrap_or(total_lines).min(total_lines);

        if start > end {
            return ToolResult::error(
                ctx.call_id,
                format!(
                    "invalid line range: start_line={start} > end_line={end} (file has {total_lines} lines)"
                ),
            );
        }

        // Format with line numbers
        let width = end.to_string().len();
        let mut output = String::new();
        let mut char_count = 0;
        let mut truncated = false;

        for (i, line) in lines[start - 1..end].iter().enumerate() {
            let line_num = start + i;
            let formatted = format!("{line_num:>width$}: {line}\n");
            char_count += formatted.len();
            if char_count > ctx.max_read_file_chars {
                truncated = true;
                break;
            }
            output.push_str(&formatted);
        }

        if truncated {
            output.push_str(&format!(
                "\n... (truncated at {} chars, file has {} lines total)\n",
                ctx.max_read_file_chars, total_lines
            ));
        }

        ToolResult::ok(ctx.call_id, output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    fn test_ctx(workspace: &Path, call_id: &str, max_chars: usize) -> ToolContext {
        let (tx, _rx) = mpsc::channel(1);
        ToolContext {
            call_id: call_id.to_owned(),
            workspace_root: workspace.to_path_buf(),
            cwd: workspace.to_path_buf(),
            events: tx,
            max_read_file_chars: max_chars,
        }
    }

    #[tokio::test]
    async fn reads_file_with_line_numbers() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("hello.txt"),
            "line one\nline two\nline three\n",
        )
        .unwrap();

        let tool = ReadFileTool;
        let ctx = test_ctx(dir.path(), "c1", 10000);
        let result = tool.execute(json!({"path": "hello.txt"}), ctx).await;

        assert!(result.error.is_none());
        assert!(result.content.contains("1: line one"));
        assert!(result.content.contains("2: line two"));
        assert!(result.content.contains("3: line three"));
    }

    #[tokio::test]
    async fn rejects_path_traversal() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("safe.txt"), "ok").unwrap();

        let tool = ReadFileTool;
        let ctx = test_ctx(dir.path(), "c2", 10000);
        let result = tool.execute(json!({"path": "../../etc/passwd"}), ctx).await;

        assert!(result.is_error());
        assert!(result.error.unwrap().contains("outside the workspace"));
    }

    #[tokio::test]
    async fn truncates_at_cap() {
        let dir = TempDir::new().unwrap();
        let content = "x".repeat(200) + "\n";
        let big = content.repeat(100);
        fs::write(dir.path().join("big.txt"), &big).unwrap();

        let tool = ReadFileTool;
        let ctx = test_ctx(dir.path(), "c3", 500);
        let result = tool.execute(json!({"path": "big.txt"}), ctx).await;

        assert!(result.error.is_none());
        assert!(result.content.contains("truncated"));
        assert!(result.content.len() < 700); // some overhead for truncation message
    }

    #[tokio::test]
    async fn line_range() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("lines.txt"), "a\nb\nc\nd\ne\n").unwrap();

        let tool = ReadFileTool;
        let ctx = test_ctx(dir.path(), "c4", 10000);
        let result = tool
            .execute(
                json!({"path": "lines.txt", "start_line": 2, "end_line": 4}),
                ctx,
            )
            .await;

        assert!(result.error.is_none());
        assert!(result.content.contains("2: b"));
        assert!(result.content.contains("3: c"));
        assert!(result.content.contains("4: d"));
        assert!(!result.content.contains("1: a"));
        assert!(!result.content.contains("5: e"));
    }

    #[tokio::test]
    async fn rejects_binary_file() {
        let dir = TempDir::new().unwrap();
        let mut content = vec![0u8; 100];
        content[0] = b'M';
        content[1] = b'Z';
        fs::write(dir.path().join("binary.bin"), &content).unwrap();

        let tool = ReadFileTool;
        let ctx = test_ctx(dir.path(), "c5", 10000);
        let result = tool.execute(json!({"path": "binary.bin"}), ctx).await;

        assert!(result.is_error());
        assert!(result.error.unwrap().contains("binary file"));
    }
}
