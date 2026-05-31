//! Phase N3 — writethrough on `apply_patch`.
//!
//! After every successful patch, the apply_patch tool calls
//! [`after_edit`] with the list of files it just wrote. We then:
//!
//! 1. Open or update each file via the language server's `didOpen` /
//!    `didChange` notifications so the server type-checks fresh content.
//! 2. Wait up to `diagnostics_timeout_ms` for the server to push
//!    `publishDiagnostics`.
//! 3. Filter diagnostics to a small window around the changed lines so
//!    pre-existing issues elsewhere in the file don't pollute the result.
//! 4. Format and return them. The apply_patch tool appends the formatted
//!    block to its tool result so the model sees its breakage in the
//!    same turn it caused it.
//!
//! Behavior is opt-out via `[lsp] writethrough = false` and timeout-bounded
//! via `[lsp] diagnostics_timeout_ms` (default 750 ms).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;

use super::manager::LspManager;
use super::render;
use super::types::DiagnosticView;

/// Number of context lines around each changed line to scope diagnostics
/// reporting to. Small enough to filter noise, big enough to catch
/// neighbouring errors caused by the edit.
const DIAGNOSTIC_LINE_BUFFER: u32 = 3;

/// Outcome of a writethrough cycle.
#[derive(Debug, Clone)]
pub struct WritethroughOutcome {
    /// Diagnostics that landed in the changed-line window, formatted.
    pub diagnostics: Vec<DiagnosticView>,
    /// Number of diagnostics elsewhere in the file (outside the window).
    pub elsewhere_count: usize,
    /// True when the wait hit the timeout before all paths reported.
    pub timed_out: bool,
}

/// Spec describing one file we wrote in this turn — what the LSP needs
/// to know to type-check it.
#[derive(Debug, Clone)]
pub struct EditedFile {
    pub path: PathBuf,
    /// Full new contents of the file. Phase N3 uses
    /// `TextDocumentSyncKind::Full` for simplicity; incremental sync is a
    /// later optimization.
    pub contents: String,
    /// 1-based line numbers that were modified by this edit. Used to
    /// scope diagnostics to changed regions.
    pub changed_lines: Vec<u32>,
}

/// Track files in the LSP and pull resulting diagnostics. Returns the
/// scoped results plus how many diagnostics were filtered out.
pub async fn after_edit(
    manager: &LspManager,
    workspace_root: &Path,
    edits: &[EditedFile],
    timeout: Duration,
) -> WritethroughOutcome {
    let mut all_views: Vec<DiagnosticView> = Vec::new();
    let mut elsewhere_total = 0usize;
    let mut timed_out = false;

    for ed in edits {
        match track_and_collect(manager, workspace_root, ed, timeout).await {
            Ok((kept, elsewhere, t)) => {
                all_views.extend(kept);
                elsewhere_total += elsewhere;
                timed_out = timed_out || t;
            }
            Err(_) => {
                // Errors during writethrough are non-fatal — the patch
                // already landed. Surface as "(timeout)" so the caller's
                // formatter does the right thing without an exception.
                timed_out = true;
            }
        }
    }

    WritethroughOutcome {
        diagnostics: all_views,
        elsewhere_count: elsewhere_total,
        timed_out,
    }
}

async fn track_and_collect(
    manager: &LspManager,
    workspace_root: &Path,
    ed: &EditedFile,
    timeout: Duration,
) -> Result<(Vec<DiagnosticView>, usize, bool)> {
    let arc_client = match manager_for_path(manager, &ed.path, workspace_root).await {
        Some(c) => c,
        None => return Ok((Vec::new(), 0, false)),
    };

    {
        let client = arc_client.lock().await;
        if let Err(e) = client.track(&ed.path, &ed.contents).await {
            tracing::debug!(target: "lsp", "track failed for {}: {e:#}", ed.path.display());
            return Ok((Vec::new(), 0, true));
        }
    }

    // Poll the diagnostics cache until either (a) the cache has at least
    // one publishDiagnostics for this path *after* track returned, or
    // (b) the timeout fires. Polling beats latching because rust-analyzer
    // pushes incrementally — first an empty list, then a populated one as
    // type-check finishes.
    let deadline = Instant::now() + timeout;
    let mut last_seen: Option<Vec<lsp_types::Diagnostic>> = None;
    let mut tick_counter: u32 = 0;
    while Instant::now() < deadline {
        let snapshot = {
            let client = arc_client.lock().await;
            client.cached_diagnostics(Some(&ed.path)).await
        };
        if let Some((_, diags)) = snapshot.into_iter().next() {
            last_seen = Some(diags);
            // Heuristic: if we got at least one diagnostic, return early.
            // If we got an empty list, wait one more poll cycle in case
            // the server is still about to push errors.
            if last_seen.as_ref().map(|v| !v.is_empty()).unwrap_or(false) {
                break;
            }
            // Empty list seen — still wait for a couple more ticks so
            // we don't claim "(clean)" prematurely while indexing.
            if tick_counter > 1 {
                break;
            }
        }
        tick_counter += 1;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let timed_out = Instant::now() >= deadline && last_seen.is_none();
    let raw = last_seen.unwrap_or_default();
    let views = render::diagnostic_views(&ed.path, &raw);
    let (kept, elsewhere) = render::filter_diagnostics_to_changed_region(
        views,
        &ed.changed_lines,
        DIAGNOSTIC_LINE_BUFFER,
    );
    Ok((kept, elsewhere, timed_out))
}

async fn manager_for_path(
    manager: &LspManager,
    path: &Path,
    workspace_root: &Path,
) -> Option<std::sync::Arc<tokio::sync::Mutex<super::client::LspClient>>> {
    // Lookup-only — writethrough should never spawn fresh servers (would
    // make the writethrough latency unbounded). If no client exists for
    // this path's resolved (server, root), the writethrough silently
    // skips — the file's diagnostics will surface the next time the
    // user runs `lsp diagnostics`.
    let (id, root) = manager.registry().resolve(path, workspace_root)?;
    manager.get_or_spawn_existing(id, root).await
}

/// Format a writethrough outcome as the trailing block apply_patch
/// appends to its tool result.
pub fn format_outcome(outcome: &WritethroughOutcome, workspace_root: &Path) -> String {
    let mut out = String::from("\n\n── LSP diagnostics ──\n");
    if outcome.diagnostics.is_empty() && !outcome.timed_out {
        out.push_str("(clean)");
        if outcome.elsewhere_count > 0 {
            out.push_str(&format!(
                "\n({} pre-existing diagnostic{} elsewhere — run `lsp diagnostics` for full list)",
                outcome.elsewhere_count,
                if outcome.elsewhere_count == 1 {
                    ""
                } else {
                    "s"
                }
            ));
        }
        return out;
    }
    if outcome.timed_out && outcome.diagnostics.is_empty() {
        out.push_str("(timeout — try `lsp diagnostics` later)");
        return out;
    }
    out.push_str(&render::format_diagnostics(
        &outcome.diagnostics,
        workspace_root,
    ));
    if outcome.elsewhere_count > 0 {
        out.push_str(&format!(
            "\n\n({} more elsewhere — run `lsp diagnostics` for full list)",
            outcome.elsewhere_count
        ));
    }
    if outcome.timed_out {
        out.push_str(
            "\n\n(partial — server hadn't finished checking when the writethrough timeout fired)",
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::types::DiagnosticView;

    #[test]
    fn format_outcome_clean() {
        let outcome = WritethroughOutcome {
            diagnostics: Vec::new(),
            elsewhere_count: 0,
            timed_out: false,
        };
        let out = format_outcome(&outcome, Path::new("/work"));
        assert!(out.contains("(clean)"), "got: {out}");
    }

    #[test]
    fn format_outcome_timeout_no_results() {
        let outcome = WritethroughOutcome {
            diagnostics: Vec::new(),
            elsewhere_count: 0,
            timed_out: true,
        };
        let out = format_outcome(&outcome, Path::new("/work"));
        assert!(out.contains("(timeout"), "got: {out}");
    }

    #[test]
    fn format_outcome_with_diagnostics() {
        let outcome = WritethroughOutcome {
            diagnostics: vec![DiagnosticView {
                path: PathBuf::from("/work/src/foo.rs"),
                line: 12,
                column: 8,
                severity: "error",
                message: "expected `;`, found `}`".to_owned(),
            }],
            elsewhere_count: 2,
            timed_out: false,
        };
        let out = format_outcome(&outcome, Path::new("/work"));
        assert!(out.contains("src/foo.rs:12:8 [error]"), "got: {out}");
        assert!(out.contains("2 more elsewhere"), "got: {out}");
    }

    #[test]
    fn format_outcome_clean_with_elsewhere() {
        let outcome = WritethroughOutcome {
            diagnostics: Vec::new(),
            elsewhere_count: 1,
            timed_out: false,
        };
        let out = format_outcome(&outcome, Path::new("/work"));
        assert!(out.contains("(clean)"), "got: {out}");
        assert!(out.contains("1 pre-existing"), "got: {out}");
    }

    #[test]
    fn format_outcome_partial_with_diagnostics_and_timeout() {
        let outcome = WritethroughOutcome {
            diagnostics: vec![DiagnosticView {
                path: PathBuf::from("/work/src/foo.rs"),
                line: 1,
                column: 1,
                severity: "warn",
                message: "unused".to_owned(),
            }],
            elsewhere_count: 0,
            timed_out: true,
        };
        let out = format_outcome(&outcome, Path::new("/work"));
        assert!(out.contains("[warn] unused"), "got: {out}");
        assert!(out.contains("partial"), "got: {out}");
    }
}
