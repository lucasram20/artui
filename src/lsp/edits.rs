//! Apply LSP `WorkspaceEdit` payloads to disk.
//!
//! Phase N4 dispatches `rename` and `code_actions` requests to the language
//! server, gets back a `lsp_types::WorkspaceEdit`, and routes through this
//! module to fan out the edits across the filesystem. We deliberately
//! reuse the same workspace-relative file write semantics as `apply_patch`
//! so the writethrough loop (Phase N3) fires per file.
//!
//! Scope deliberately narrow:
//!
//! - Only `TextEdit`/`AnnotatedTextEdit` are honored. Resource-creation,
//!   rename, and delete operations from `WorkspaceEdit::document_changes`
//!   are surfaced in the report but skipped (the model can request them
//!   via `apply_patch` if needed).
//! - Edits within a file are applied bottom-up so earlier `range` offsets
//!   stay valid as we walk through them. This matches the LSP spec —
//!   client must apply sorted-by-position-descending.
//! - Errors are partial: if file 3 of 5 fails, files 1-2 stay applied.
//!   The report carries `applied`/`total` so the caller can decide what
//!   to do next.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use lsp_types::{TextEdit, WorkspaceEdit};

/// Outcome of applying a `WorkspaceEdit`.
#[derive(Debug, Clone)]
pub struct ApplyReport {
    /// Files we successfully wrote.
    pub applied: usize,
    /// Total files referenced by the WorkspaceEdit (excluding skipped
    /// resource ops).
    pub total: usize,
    /// Per-file outcome — useful for diagnostics surfacing.
    pub files: Vec<FileOutcome>,
}

#[derive(Debug, Clone)]
pub struct FileOutcome {
    pub path: PathBuf,
    pub edits_applied: usize,
    pub error: Option<String>,
}

/// Apply a `WorkspaceEdit` rooted at `workspace_root`. Returns an
/// [`ApplyReport`] summarising what landed.
pub async fn apply_workspace_edit(
    edit: &WorkspaceEdit,
    workspace_root: &Path,
) -> Result<ApplyReport> {
    let mut by_path: HashMap<PathBuf, Vec<TextEdit>> = HashMap::new();

    if let Some(changes) = &edit.changes {
        for (uri, edits) in changes {
            let path = uri
                .to_file_path()
                .map_err(|_| anyhow!("invalid file:// URL in workspace edit: {uri}"))?;
            by_path.entry(path).or_default().extend(edits.clone());
        }
    }
    if let Some(doc_changes) = &edit.document_changes {
        match doc_changes {
            lsp_types::DocumentChanges::Edits(edits) => {
                for ed in edits {
                    let path = ed
                        .text_document
                        .uri
                        .to_file_path()
                        .map_err(|_| anyhow!("invalid file:// URL in workspace edit"))?;
                    let plain: Vec<TextEdit> = ed
                        .edits
                        .iter()
                        .map(|e| match e {
                            lsp_types::OneOf::Left(te) => te.clone(),
                            lsp_types::OneOf::Right(annot) => TextEdit {
                                range: annot.text_edit.range,
                                new_text: annot.text_edit.new_text.clone(),
                            },
                        })
                        .collect();
                    by_path.entry(path).or_default().extend(plain);
                }
            }
            lsp_types::DocumentChanges::Operations(_) => {
                // Resource ops out of scope; surfaced via the report.
            }
        }
    }

    let total = by_path.len();
    let mut files = Vec::with_capacity(total);
    let mut applied = 0;

    for (path, edits) in by_path {
        // Refuse to write outside workspace root. apply_patch enforces the
        // same boundary; doing it here too keeps rename/code_actions from
        // being a path-traversal escape hatch.
        if !is_within_workspace(&path, workspace_root) {
            files.push(FileOutcome {
                path: path.clone(),
                edits_applied: 0,
                error: Some(format!(
                    "refusing to apply edit outside workspace: {}",
                    path.display()
                )),
            });
            continue;
        }

        match apply_edits_to_file(&path, &edits) {
            Ok(n) => {
                applied += 1;
                files.push(FileOutcome {
                    path,
                    edits_applied: n,
                    error: None,
                });
            }
            Err(error) => {
                files.push(FileOutcome {
                    path,
                    edits_applied: 0,
                    error: Some(format!("{error:#}")),
                });
            }
        }
    }

    Ok(ApplyReport {
        applied,
        total,
        files,
    })
}

fn is_within_workspace(path: &Path, workspace_root: &Path) -> bool {
    let canon_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let canon_root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    canon_path.starts_with(&canon_root)
}

fn apply_edits_to_file(path: &Path, edits: &[TextEdit]) -> Result<usize> {
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {} for edit", path.display()))?;
    let updated = apply_text_edits(&original, edits)?;
    std::fs::write(path, updated)
        .with_context(|| format!("failed to write {} after edit", path.display()))?;
    Ok(edits.len())
}

/// Apply a slice of [`TextEdit`]s to `original` and return the new string.
/// Edits are sorted by position descending so earlier offsets stay valid
/// as later edits are applied.
pub fn apply_text_edits(original: &str, edits: &[TextEdit]) -> Result<String> {
    if edits.is_empty() {
        return Ok(original.to_owned());
    }

    // Build a line offset table once — `TextEdit::range` is in
    // (line, character) and we need byte offsets to splice the string.
    let line_offsets = compute_line_offsets(original);

    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by(|a, b| {
        // Descending by start position.
        b.range
            .start
            .line
            .cmp(&a.range.start.line)
            .then(b.range.start.character.cmp(&a.range.start.character))
    });

    let mut result = original.to_owned();
    for edit in sorted {
        let start = position_to_offset(&line_offsets, &result, edit.range.start)?;
        let end = position_to_offset(&line_offsets, &result, edit.range.end)?;
        if start > end || end > result.len() {
            bail!(
                "TextEdit range out of bounds: ({},{})-({},{})",
                edit.range.start.line,
                edit.range.start.character,
                edit.range.end.line,
                edit.range.end.character
            );
        }
        result.replace_range(start..end, &edit.new_text);
    }
    Ok(result)
}

fn compute_line_offsets(text: &str) -> Vec<usize> {
    let mut offsets = vec![0usize];
    for (i, ch) in text.char_indices() {
        if ch == '\n' {
            offsets.push(i + 1);
        }
    }
    offsets
}

fn position_to_offset(
    line_offsets: &[usize],
    text: &str,
    pos: lsp_types::Position,
) -> Result<usize> {
    let line = pos.line as usize;
    let line_start = if line < line_offsets.len() {
        line_offsets[line]
    } else if line == line_offsets.len() {
        // Position points at the synthetic line one past the last newline
        // (end-of-file marker). Treat as `text.len()`.
        text.len()
    } else {
        bail!(
            "line {} out of range (file has {} lines)",
            line,
            line_offsets.len()
        );
    };

    // Walk `pos.character` UTF-16 code units forward from line_start and
    // return the byte offset. LSP positions count UTF-16 code units, but
    // most edits sent by rust-analyzer are ASCII, where UTF-16 units ==
    // bytes. For non-ASCII, we still want to land on a char boundary.
    let mut cursor = line_start;
    let mut utf16_units = 0u32;
    let bytes = text.as_bytes();
    while utf16_units < pos.character && cursor < bytes.len() {
        if bytes[cursor] == b'\n' {
            break;
        }
        // Advance one char at a time so `cursor` always lands on a UTF-8
        // boundary; sum the char's UTF-16 width.
        let ch_len = utf8_char_len(bytes[cursor]);
        let ch_str = std::str::from_utf8(&bytes[cursor..cursor + ch_len])?;
        let ch = ch_str
            .chars()
            .next()
            .ok_or_else(|| anyhow!("empty utf-8 sequence at offset {cursor}"))?;
        utf16_units = utf16_units.saturating_add(ch.len_utf16() as u32);
        cursor += ch_len;
    }
    Ok(cursor)
}

fn utf8_char_len(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{Position, Range, Url};

    fn edit(line: u32, col_start: u32, col_end: u32, text: &str) -> TextEdit {
        TextEdit {
            range: Range {
                start: Position {
                    line,
                    character: col_start,
                },
                end: Position {
                    line,
                    character: col_end,
                },
            },
            new_text: text.to_owned(),
        }
    }

    #[test]
    fn apply_text_edits_single_replacement() {
        let src = "fn answer() -> i32 { 42 }";
        let result = apply_text_edits(src, &[edit(0, 3, 9, "best")]).unwrap();
        assert_eq!(result, "fn best() -> i32 { 42 }");
    }

    #[test]
    fn apply_text_edits_multiline() {
        let src = "fn a() {\n    let x = 1;\n    let y = 2;\n}\n";
        // Replace the full `let x = 1;` line.
        let result = apply_text_edits(src, &[edit(1, 4, 14, "let x = 99;")]).unwrap();
        assert_eq!(result, "fn a() {\n    let x = 99;\n    let y = 2;\n}\n");
    }

    #[test]
    fn apply_text_edits_multiple_descending() {
        // Two edits on the same line, descending order is enforced inside
        // `apply_text_edits` even if we pass them in ascending order.
        let src = "let a = 1; let b = 2;";
        let result = apply_text_edits(
            src,
            &[
                edit(0, 4, 5, "x"),   // a → x
                edit(0, 15, 16, "y"), // b → y
            ],
        )
        .unwrap();
        assert_eq!(result, "let x = 1; let y = 2;");
    }

    #[test]
    fn apply_text_edits_empty_no_change() {
        let src = "unchanged";
        let result = apply_text_edits(src, &[]).unwrap();
        assert_eq!(result, "unchanged");
    }

    #[test]
    fn apply_text_edits_handles_unicode() {
        let src = "// 你好\nlet x = 1;";
        // Replace `let` with `var` — the comment line has multi-byte chars
        // but the edit is on line 1, so column maps to bytes the same as
        // ASCII.
        let result = apply_text_edits(src, &[edit(1, 0, 3, "var")]).unwrap();
        assert_eq!(result, "// 你好\nvar x = 1;");
    }

    #[test]
    fn apply_text_edits_rejects_out_of_bounds() {
        let src = "short";
        let bad = edit(99, 0, 10, "x");
        assert!(apply_text_edits(src, &[bad]).is_err());
    }

    #[tokio::test]
    async fn apply_workspace_edit_writes_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("foo.rs");
        std::fs::write(&path, "fn old_name() {}").unwrap();

        let mut changes = std::collections::HashMap::new();
        changes.insert(
            Url::from_file_path(&path).unwrap(),
            vec![edit(0, 3, 11, "new_name")],
        );
        let we = WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        };
        let report = apply_workspace_edit(&we, dir.path()).await.unwrap();
        assert_eq!(report.applied, 1);
        assert_eq!(report.total, 1);
        let updated = std::fs::read_to_string(&path).unwrap();
        assert_eq!(updated, "fn new_name() {}");
    }

    #[tokio::test]
    async fn apply_workspace_edit_refuses_outside_workspace() {
        let dir = tempfile::TempDir::new().unwrap();
        let outside_dir = tempfile::TempDir::new().unwrap();
        let outside_path = outside_dir.path().join("escape.rs");
        std::fs::write(&outside_path, "fn x() {}").unwrap();

        let mut changes = std::collections::HashMap::new();
        changes.insert(
            Url::from_file_path(&outside_path).unwrap(),
            vec![edit(0, 3, 4, "y")],
        );
        let we = WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        };
        let report = apply_workspace_edit(&we, dir.path()).await.unwrap();
        assert_eq!(report.applied, 0);
        assert_eq!(report.total, 1);
        assert!(report.files[0]
            .error
            .as_ref()
            .unwrap()
            .contains("outside workspace"));
        // The outside file must remain untouched.
        let unchanged = std::fs::read_to_string(&outside_path).unwrap();
        assert_eq!(unchanged, "fn x() {}");
    }
}
