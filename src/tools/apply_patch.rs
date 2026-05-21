//! `apply_patch` tool — V4A format patch parser and applier.
//!
//! V4A format:
//! ```text
//! *** Begin Patch
//! *** Update File: path/to/file.rs
//! @@ context line
//! -old line
//! +new line
//! *** Add File: path/to/new_file.rs
//! +line 1
//! +line 2
//! *** Delete File: path/to/old_file.rs
//! *** End Patch
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::providers::ToolSpec;

use super::{Tool, ToolContext, ToolResult};

pub struct ApplyPatchTool;

#[async_trait]
impl Tool for ApplyPatchTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "apply_patch".to_owned(),
            description: "Apply a V4A-format patch to files in the workspace. Supports Update, Add, and Delete operations.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "patch": {
                        "type": "string",
                        "description": "The patch content in V4A format (*** Begin Patch ... *** End Patch)"
                    }
                },
                "required": ["patch"]
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: ToolContext) -> ToolResult {
        let Some(patch_str) = args.get("patch").and_then(|v| v.as_str()) else {
            return ToolResult::error(ctx.call_id, "missing required parameter: patch".to_owned());
        };

        // Parse the patch
        let operations = match parse_v4a(patch_str) {
            Ok(ops) => ops,
            Err(e) => return ToolResult::error(ctx.call_id, format!("patch parse error: {e}")),
        };

        if operations.is_empty() {
            return ToolResult::error(ctx.call_id, "patch contains no operations".to_owned());
        }

        // Validate all paths are within workspace
        for op in &operations {
            let path = op.path();
            if path.is_absolute() || path.starts_with("..") {
                return ToolResult::error(
                    ctx.call_id,
                    format!("path '{}' is outside the workspace", path.display()),
                );
            }
            let resolved = ctx.workspace_root.join(path);
            if let Ok(canonical) = resolved.canonicalize() {
                if !canonical.starts_with(&ctx.workspace_root) {
                    return ToolResult::error(
                        ctx.call_id,
                        format!("path '{}' resolves outside the workspace", path.display()),
                    );
                }
            }
        }

        // Apply atomically: collect all changes first, then write
        let mut changes: Vec<FileChange> = Vec::new();

        for op in &operations {
            match op {
                PatchOp::Add { path, content } => {
                    let target = ctx.workspace_root.join(path);
                    if target.exists() {
                        return ToolResult::error(
                            ctx.call_id,
                            format!("cannot add '{}': file already exists", path.display()),
                        );
                    }
                    changes.push(FileChange::Create {
                        path: target,
                        content: content.clone(),
                    });
                }
                PatchOp::Delete { path } => {
                    let target = ctx.workspace_root.join(path);
                    if !target.exists() {
                        return ToolResult::error(
                            ctx.call_id,
                            format!("cannot delete '{}': file does not exist", path.display()),
                        );
                    }
                    let original = fs::read_to_string(&target).unwrap_or_default();
                    changes.push(FileChange::Delete {
                        path: target,
                        original,
                    });
                }
                PatchOp::Update { path, hunks } => {
                    let target = ctx.workspace_root.join(path);
                    if !target.exists() {
                        return ToolResult::error(
                            ctx.call_id,
                            format!("cannot update '{}': file does not exist", path.display()),
                        );
                    }
                    let original = match fs::read_to_string(&target) {
                        Ok(s) => s,
                        Err(e) => {
                            return ToolResult::error(
                                ctx.call_id,
                                format!("cannot read '{}': {e}", path.display()),
                            );
                        }
                    };
                    let new_content = match apply_hunks(&original, hunks) {
                        Ok(s) => s,
                        Err(e) => {
                            let lines: Vec<&str> = original.lines().collect();
                            let preview = lines
                                .iter()
                                .take(20)
                                .enumerate()
                                .map(|(i, l)| format!("{}: {l}", i + 1))
                                .collect::<Vec<_>>()
                                .join("\n");
                            return ToolResult::error(
                                ctx.call_id,
                                format!(
                                    "patch failed on '{}': {e}\nFirst 20 lines:\n{preview}",
                                    path.display()
                                ),
                            );
                        }
                    };
                    changes.push(FileChange::Update {
                        path: target,
                        original,
                        new_content,
                    });
                }
            }
        }

        // Execute all changes
        let mut applied: Vec<&FileChange> = Vec::new();
        for change in &changes {
            if let Err(e) = execute_change(change) {
                for prev in applied.iter().rev() {
                    let _ = rollback_change(prev);
                }
                return ToolResult::error(
                    ctx.call_id,
                    format!("failed to apply patch: {e}; rolled back"),
                );
            }
            applied.push(change);
        }

        let summary = changes
            .iter()
            .map(|c| match c {
                FileChange::Create { path, .. } => format!("+ {}", path.display()),
                FileChange::Delete { path, .. } => format!("- {}", path.display()),
                FileChange::Update { path, .. } => format!("~ {}", path.display()),
            })
            .collect::<Vec<_>>()
            .join("\n");

        ToolResult::ok(
            ctx.call_id,
            format!("Patch applied successfully:\n{summary}"),
        )
    }
}

// ---------------------------------------------------------------------------
// V4A Parser
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum PatchOp {
    Add { path: PathBuf, content: String },
    Delete { path: PathBuf },
    Update { path: PathBuf, hunks: Vec<Hunk> },
}

impl PatchOp {
    fn path(&self) -> &Path {
        match self {
            Self::Add { path, .. } => path,
            Self::Delete { path } => path,
            Self::Update { path, .. } => path,
        }
    }
}

#[derive(Debug, Clone)]
struct Hunk {
    context: Vec<String>,
    removals: Vec<String>,
    additions: Vec<String>,
}

fn parse_v4a(input: &str) -> Result<Vec<PatchOp>, String> {
    let lines: Vec<&str> = input.lines().collect();
    let mut ops = Vec::new();
    let mut i = 0;

    // Find *** Begin Patch
    while i < lines.len() {
        if lines[i].trim() == "*** Begin Patch" {
            i += 1;
            break;
        }
        i += 1;
    }

    while i < lines.len() {
        let line = lines[i].trim();

        if line == "*** End Patch" {
            break;
        }

        if let Some(path) = line.strip_prefix("*** Add File:") {
            let path = PathBuf::from(path.trim());
            i += 1;
            let mut content = String::new();
            while i < lines.len() {
                let l = lines[i];
                if l.starts_with("*** ") {
                    break;
                }
                if let Some(added) = l.strip_prefix('+') {
                    content.push_str(added);
                    content.push('\n');
                } else if l.trim().is_empty() {
                    content.push('\n');
                }
                i += 1;
            }
            ops.push(PatchOp::Add { path, content });
        } else if let Some(path) = line.strip_prefix("*** Delete File:") {
            let path = PathBuf::from(path.trim());
            i += 1;
            ops.push(PatchOp::Delete { path });
        } else if let Some(path) = line.strip_prefix("*** Update File:") {
            let path = PathBuf::from(path.trim());
            i += 1;
            let mut hunks = Vec::new();

            while i < lines.len() {
                let l = lines[i];
                if l.starts_with("*** ") {
                    break;
                }

                if l.starts_with("@@") {
                    i += 1;
                    let mut hunk = Hunk {
                        context: Vec::new(),
                        removals: Vec::new(),
                        additions: Vec::new(),
                    };

                    let ctx_line = l.strip_prefix("@@").unwrap_or("").trim();
                    if !ctx_line.is_empty() {
                        hunk.context.push(ctx_line.to_owned());
                    }

                    while i < lines.len() {
                        let hl = lines[i];
                        if hl.starts_with("@@") || hl.starts_with("*** ") {
                            break;
                        }
                        if let Some(removed) = hl.strip_prefix('-') {
                            hunk.removals.push(removed.to_owned());
                        } else if let Some(added) = hl.strip_prefix('+') {
                            hunk.additions.push(added.to_owned());
                        } else if let Some(ctx) = hl.strip_prefix(' ') {
                            hunk.context.push(ctx.to_owned());
                        } else if !hl.trim().is_empty() {
                            hunk.context.push(hl.to_owned());
                        }
                        i += 1;
                    }

                    hunks.push(hunk);
                } else {
                    i += 1;
                }
            }

            ops.push(PatchOp::Update { path, hunks });
        } else {
            i += 1;
        }
    }

    Ok(ops)
}

// ---------------------------------------------------------------------------
// Hunk application
// ---------------------------------------------------------------------------

fn apply_hunks(original: &str, hunks: &[Hunk]) -> Result<String, String> {
    let mut lines: Vec<String> = original.lines().map(|l| l.to_owned()).collect();

    for (hunk_idx, hunk) in hunks.iter().enumerate() {
        let pos = find_hunk_position(&lines, hunk).ok_or_else(|| {
            format!(
                "hunk {} could not be located (context: {:?})",
                hunk_idx + 1,
                hunk.context.first().unwrap_or(&String::new())
            )
        })?;

        let remove_count = hunk.removals.len();

        // Verify removals match
        for (j, removal) in hunk.removals.iter().enumerate() {
            let line_idx = pos + j;
            if line_idx >= lines.len() {
                return Err(format!(
                    "hunk {}: expected line '{}' at position {} but file ended",
                    hunk_idx + 1,
                    removal,
                    line_idx + 1
                ));
            }
            if lines[line_idx].trim_end() != removal.trim_end() {
                return Err(format!(
                    "hunk {}: expected '{}' at line {} but found '{}'",
                    hunk_idx + 1,
                    removal,
                    line_idx + 1,
                    lines[line_idx]
                ));
            }
        }

        // Apply: remove old lines, insert new ones
        let drain_end = (pos + remove_count).min(lines.len());
        lines.drain(pos..drain_end);
        for (j, addition) in hunk.additions.iter().enumerate() {
            lines.insert(pos + j, addition.clone());
        }
    }

    let mut result = lines.join("\n");
    if original.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    Ok(result)
}

fn find_hunk_position(lines: &[String], hunk: &Hunk) -> Option<usize> {
    let search_line = if !hunk.removals.is_empty() {
        &hunk.removals[0]
    } else if !hunk.context.is_empty() {
        &hunk.context[0]
    } else {
        return Some(lines.len());
    };

    // Exact match
    for (i, line) in lines.iter().enumerate() {
        if line.trim_end() == search_line.trim_end() && verify_context(lines, i, hunk) {
            return Some(i);
        }
    }

    // Fuzzy: trimmed match
    let trimmed_search = search_line.trim();
    for (i, line) in lines.iter().enumerate() {
        if line.trim() == trimmed_search && verify_context(lines, i, hunk) {
            return Some(i);
        }
    }

    None
}

fn verify_context(lines: &[String], pos: usize, hunk: &Hunk) -> bool {
    for (j, removal) in hunk.removals.iter().enumerate() {
        let idx = pos + j;
        if idx >= lines.len() {
            return false;
        }
        if lines[idx].trim_end() != removal.trim_end() {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// File changes
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum FileChange {
    Create {
        path: PathBuf,
        content: String,
    },
    Delete {
        path: PathBuf,
        original: String,
    },
    Update {
        path: PathBuf,
        original: String,
        new_content: String,
    },
}

fn execute_change(change: &FileChange) -> Result<(), String> {
    match change {
        FileChange::Create { path, content } => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("cannot create directory: {e}"))?;
            }
            fs::write(path, content).map_err(|e| format!("cannot write '{}': {e}", path.display()))
        }
        FileChange::Delete { path, .. } => {
            fs::remove_file(path).map_err(|e| format!("cannot delete '{}': {e}", path.display()))
        }
        FileChange::Update {
            path, new_content, ..
        } => fs::write(path, new_content)
            .map_err(|e| format!("cannot write '{}': {e}", path.display())),
    }
}

fn rollback_change(change: &FileChange) -> Result<(), String> {
    match change {
        FileChange::Create { path, .. } => {
            let _ = fs::remove_file(path);
            Ok(())
        }
        FileChange::Delete { path, original } => fs::write(path, original)
            .map_err(|e| format!("rollback failed for '{}': {e}", path.display())),
        FileChange::Update { path, original, .. } => fs::write(path, original)
            .map_err(|e| format!("rollback failed for '{}': {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    fn test_ctx(workspace: &Path) -> ToolContext {
        let (tx, _rx) = mpsc::channel(1);
        ToolContext {
            call_id: "p1".to_owned(),
            workspace_root: workspace.to_path_buf(),
            cwd: workspace.to_path_buf(),
            events: tx,
            max_read_file_chars: 10000,
        }
    }

    #[test]
    fn parse_update_file() {
        let patch = r#"*** Begin Patch
*** Update File: src/main.rs
@@ fn main() {
-    println!("hi");
+    println!("hello");
*** End Patch"#;

        let ops = parse_v4a(patch).unwrap();
        assert_eq!(ops.len(), 1);
        assert!(
            matches!(&ops[0], PatchOp::Update { path, hunks } if path == Path::new("src/main.rs") && hunks.len() == 1)
        );
    }

    #[test]
    fn parse_add_file() {
        let patch = r#"*** Begin Patch
*** Add File: new.txt
+hello
+world
*** End Patch"#;

        let ops = parse_v4a(patch).unwrap();
        assert_eq!(ops.len(), 1);
        assert!(
            matches!(&ops[0], PatchOp::Add { path, content } if path == Path::new("new.txt") && content.contains("hello"))
        );
    }

    #[test]
    fn parse_delete_file() {
        let patch = r#"*** Begin Patch
*** Delete File: old.txt
*** End Patch"#;

        let ops = parse_v4a(patch).unwrap();
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], PatchOp::Delete { path } if path == Path::new("old.txt")));
    }

    #[test]
    fn apply_simple_hunk() {
        let original = "fn main() {\n    println!(\"hi\");\n}\n";
        let hunks = vec![Hunk {
            context: vec!["fn main() {".to_owned()],
            removals: vec!["    println!(\"hi\");".to_owned()],
            additions: vec!["    println!(\"hello\");".to_owned()],
        }];

        let result = apply_hunks(original, &hunks).unwrap();
        assert!(result.contains("println!(\"hello\")"));
        assert!(!result.contains("println!(\"hi\")"));
    }

    #[test]
    fn apply_multi_hunk() {
        let original = "line1\nline2\nline3\nline4\nline5\n";
        let hunks = vec![
            Hunk {
                context: vec![],
                removals: vec!["line2".to_owned()],
                additions: vec!["LINE2".to_owned()],
            },
            Hunk {
                context: vec![],
                removals: vec!["line4".to_owned()],
                additions: vec!["LINE4".to_owned()],
            },
        ];

        let result = apply_hunks(original, &hunks).unwrap();
        assert!(result.contains("LINE2"));
        assert!(result.contains("LINE4"));
        assert!(!result.contains("line2"));
        assert!(!result.contains("line4"));
    }

    #[tokio::test]
    async fn tool_applies_patch() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("hello.rs"),
            "fn main() {\n    println!(\"hi\");\n}\n",
        )
        .unwrap();

        let tool = ApplyPatchTool;
        let ctx = test_ctx(dir.path());
        let patch = r#"*** Begin Patch
*** Update File: hello.rs
@@ fn main() {
-    println!("hi");
+    println!("hello");
*** End Patch"#;

        let result = tool.execute(json!({"patch": patch}), ctx).await;
        assert!(result.error.is_none(), "error: {:?}", result.error);
        assert!(result.content.contains("applied successfully"));

        let content = fs::read_to_string(dir.path().join("hello.rs")).unwrap();
        assert!(content.contains("println!(\"hello\")"));
    }

    #[tokio::test]
    async fn tool_rejects_path_traversal() {
        let dir = TempDir::new().unwrap();

        let tool = ApplyPatchTool;
        let ctx = test_ctx(dir.path());
        let patch = r#"*** Begin Patch
*** Add File: ../escape.txt
+bad
*** End Patch"#;

        let result = tool.execute(json!({"patch": patch}), ctx).await;
        assert!(result.is_error());
        assert!(result.error.unwrap().contains("outside the workspace"));
    }

    #[tokio::test]
    async fn tool_adds_new_file() {
        let dir = TempDir::new().unwrap();

        let tool = ApplyPatchTool;
        let ctx = test_ctx(dir.path());
        let patch = r#"*** Begin Patch
*** Add File: new_file.txt
+hello world
+second line
*** End Patch"#;

        let result = tool.execute(json!({"patch": patch}), ctx).await;
        assert!(result.error.is_none());

        let content = fs::read_to_string(dir.path().join("new_file.txt")).unwrap();
        assert!(content.contains("hello world"));
        assert!(content.contains("second line"));
    }

    #[tokio::test]
    async fn tool_deletes_file() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("doomed.txt"), "bye").unwrap();

        let tool = ApplyPatchTool;
        let ctx = test_ctx(dir.path());
        let patch = r#"*** Begin Patch
*** Delete File: doomed.txt
*** End Patch"#;

        let result = tool.execute(json!({"patch": patch}), ctx).await;
        assert!(result.error.is_none());
        assert!(!dir.path().join("doomed.txt").exists());
    }

    #[tokio::test]
    async fn tool_atomic_rollback_on_failure() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.txt"), "original a\n").unwrap();

        let tool = ApplyPatchTool;
        let ctx = test_ctx(dir.path());
        let patch = r#"*** Begin Patch
*** Update File: a.txt
-original a
+modified a
*** Update File: b.txt
-nonexistent
+something
*** End Patch"#;

        let result = tool.execute(json!({"patch": patch}), ctx).await;
        assert!(result.is_error());

        // a.txt should be unchanged (rolled back)
        let content = fs::read_to_string(dir.path().join("a.txt")).unwrap();
        assert_eq!(content, "original a\n");
    }
}
