//! Rendering helpers — turn `lsp_types` payloads into strings the model
//! can act on without further parsing.

use std::collections::BTreeMap;
use std::path::Path;

use lsp_types::{
    Diagnostic, DiagnosticSeverity, DocumentSymbol, DocumentSymbolResponse, GotoDefinitionResponse,
    Hover, HoverContents, Location, LocationLink, MarkedString, Range, SymbolInformation,
    SymbolKind, TextEdit, Url, WorkspaceEdit,
};

use super::types::{DiagnosticView, HoverView, LocationView, SymbolView};

/// Maximum hits we hand back to the model for a definition / implementation /
/// type-definition response. Mirrors oh-my-pi's cap.
pub const MAX_LOCATION_HITS: usize = 8;

/// Convert `textDocument/definition`'s response into a list of [`LocationView`]
/// entries. Handles all three serde variants the protocol can return.
pub fn locations_from_response(response: GotoDefinitionResponse) -> Vec<LocationView> {
    let mut out = Vec::new();
    match response {
        GotoDefinitionResponse::Scalar(loc) => push_location(&mut out, &loc),
        GotoDefinitionResponse::Array(locs) => {
            for loc in locs {
                push_location(&mut out, &loc);
                if out.len() >= MAX_LOCATION_HITS {
                    break;
                }
            }
        }
        GotoDefinitionResponse::Link(links) => {
            for link in links {
                push_link(&mut out, &link);
                if out.len() >= MAX_LOCATION_HITS {
                    break;
                }
            }
        }
    }
    out.truncate(MAX_LOCATION_HITS);
    out
}

fn push_location(out: &mut Vec<LocationView>, loc: &Location) {
    if let Some(view) = location_view(&loc.uri, &loc.range) {
        out.push(view);
    }
}

fn push_link(out: &mut Vec<LocationView>, link: &LocationLink) {
    if let Some(view) = location_view(&link.target_uri, &link.target_selection_range) {
        out.push(view);
    }
}

fn location_view(uri: &Url, range: &Range) -> Option<LocationView> {
    let path = uri.to_file_path().ok()?;
    Some(LocationView {
        path,
        line: range.start.line.saturating_add(1),
        column: range.start.character.saturating_add(1),
        preview: None,
    })
}

/// Format a list of [`LocationView`] entries as one line per hit, paths made
/// relative to `workspace_root` when possible. References-style entries
/// with a `preview` field render the preview line indented.
pub fn format_locations(locations: &[LocationView], workspace_root: &Path) -> String {
    if locations.is_empty() {
        return "no definition found".to_owned();
    }
    let mut out = String::new();
    for (i, loc) in locations.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let display = relativize(&loc.path, workspace_root);
        out.push_str(&format!("{display}:{}:{}", loc.line, loc.column));
        if let Some(preview) = &loc.preview {
            out.push_str("\n  ");
            out.push_str(preview.trim());
        }
    }
    out
}

/// Cap on `references` results — same shape as `MAX_LOCATION_HITS` for
/// the `definition` family, but separately tunable since reference sets
/// can be much larger (a popular trait can have hundreds of usages).
pub const MAX_REFERENCES_HITS: usize = 50;

/// Convert raw `lsp_types::Location`s (from `references`) into
/// [`LocationView`]s with preview populated where possible.
///
/// `read_preview` is a callback that returns the line content at
/// `(path, 1-based-line)`. Tests pass a stub; the production caller
/// reads from disk via `std::fs::read_to_string` + `lines().nth()`.
pub fn references_from_locations<F>(
    locations: Vec<Location>,
    mut read_preview: F,
) -> Vec<LocationView>
where
    F: FnMut(&Path, u32) -> Option<String>,
{
    let mut out = Vec::with_capacity(locations.len().min(MAX_REFERENCES_HITS));
    for loc in locations {
        if out.len() >= MAX_REFERENCES_HITS {
            break;
        }
        let Some(path) = loc.uri.to_file_path().ok() else {
            continue;
        };
        let line = loc.range.start.line.saturating_add(1);
        let column = loc.range.start.character.saturating_add(1);
        let preview = read_preview(&path, line);
        out.push(LocationView {
            path,
            line,
            column,
            preview,
        });
    }
    out
}

/// Format a list of references with the same `path:line:col` header but
/// a 2-space-indented preview line under each.
pub fn format_references(views: &[LocationView], workspace_root: &Path, total: usize) -> String {
    if views.is_empty() {
        return "no references found".to_owned();
    }
    let mut out = format_locations(views, workspace_root);
    if total > views.len() {
        out.push_str(&format!(
            "\n\n(showing {} of {} references — narrow with workspace_symbols query)",
            views.len(),
            total
        ));
    }
    out
}

// ── Symbol rendering (Phase N2) ──────────────────────────────────────────

/// Map an LSP `SymbolKind` to a short, model-friendly tag. Mirrors what an
/// editor's outline view shows.
pub fn symbol_kind_tag(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::FILE => "file",
        SymbolKind::MODULE => "mod",
        SymbolKind::NAMESPACE => "ns",
        SymbolKind::PACKAGE => "pkg",
        SymbolKind::CLASS => "class",
        SymbolKind::METHOD => "fn",
        SymbolKind::PROPERTY => "prop",
        SymbolKind::FIELD => "field",
        SymbolKind::CONSTRUCTOR => "ctor",
        SymbolKind::ENUM => "enum",
        SymbolKind::INTERFACE => "trait",
        SymbolKind::FUNCTION => "fn",
        SymbolKind::VARIABLE => "var",
        SymbolKind::CONSTANT => "const",
        SymbolKind::STRING => "str",
        SymbolKind::NUMBER => "num",
        SymbolKind::BOOLEAN => "bool",
        SymbolKind::ARRAY => "array",
        SymbolKind::OBJECT => "obj",
        SymbolKind::KEY => "key",
        SymbolKind::NULL => "null",
        SymbolKind::ENUM_MEMBER => "variant",
        SymbolKind::STRUCT => "struct",
        SymbolKind::EVENT => "event",
        SymbolKind::OPERATOR => "op",
        SymbolKind::TYPE_PARAMETER => "tparam",
        _ => "?",
    }
}

/// Flatten a `documentSymbol` response into [`SymbolView`] entries with
/// indentation depth set so the renderer can emit a tree.
pub fn flatten_document_symbols(response: DocumentSymbolResponse) -> Vec<SymbolView> {
    let mut out = Vec::new();
    match response {
        DocumentSymbolResponse::Flat(items) => {
            for item in items {
                let pos = item.location.range.start;
                out.push(SymbolView {
                    name: item.name,
                    kind: symbol_kind_tag(item.kind),
                    depth: 0,
                    line: pos.line.saturating_add(1),
                    column: pos.character.saturating_add(1),
                });
            }
        }
        DocumentSymbolResponse::Nested(items) => {
            for item in items {
                walk_symbol(&item, 0, &mut out);
            }
        }
    }
    out
}

fn walk_symbol(sym: &DocumentSymbol, depth: u32, out: &mut Vec<SymbolView>) {
    out.push(SymbolView {
        name: sym.name.clone(),
        kind: symbol_kind_tag(sym.kind),
        depth,
        line: sym.selection_range.start.line.saturating_add(1),
        column: sym.selection_range.start.character.saturating_add(1),
    });
    if let Some(children) = &sym.children {
        for child in children {
            walk_symbol(child, depth + 1, out);
        }
    }
}

/// Format a list of [`SymbolView`]s as a tree:
///
///     [struct] App
///       [fn] new
///       [fn] handle_event
///     [fn] format_locations
pub fn format_document_symbols(symbols: &[SymbolView]) -> String {
    if symbols.is_empty() {
        return "(no symbols)".to_owned();
    }
    let mut out = String::new();
    for (i, sym) in symbols.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        for _ in 0..sym.depth {
            out.push_str("  ");
        }
        out.push_str(&format!(
            "[{}] {}  ({}:{})",
            sym.kind, sym.name, sym.line, sym.column
        ));
    }
    out
}

/// Cap on `workspace_symbols` results — large queries can return thousands.
pub const MAX_WORKSPACE_SYMBOL_HITS: usize = 50;

/// Render a `workspace/symbol` flat-list response.
#[allow(deprecated)]
pub fn format_workspace_symbols(symbols: &[SymbolInformation], workspace_root: &Path) -> String {
    if symbols.is_empty() {
        return "(no matching symbols)".to_owned();
    }
    let mut out = String::new();
    for (i, sym) in symbols.iter().take(MAX_WORKSPACE_SYMBOL_HITS).enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let path = sym
            .location
            .uri
            .to_file_path()
            .map(|p| relativize(&p, workspace_root))
            .unwrap_or_else(|_| sym.location.uri.to_string());
        let pos = sym.location.range.start;
        out.push_str(&format!(
            "[{}] {}  ({}:{}:{})",
            symbol_kind_tag(sym.kind),
            sym.name,
            path,
            pos.line.saturating_add(1),
            pos.character.saturating_add(1),
        ));
    }
    if symbols.len() > MAX_WORKSPACE_SYMBOL_HITS {
        out.push_str(&format!(
            "\n\n(showing {} of {} — narrow `query` to filter)",
            MAX_WORKSPACE_SYMBOL_HITS,
            symbols.len()
        ));
    }
    out
}

// ── Diagnostics rendering (Phase N2 + N3) ────────────────────────────────

/// Convert an LSP `DiagnosticSeverity` into a short tag.
pub fn diagnostic_severity_tag(sev: Option<DiagnosticSeverity>) -> &'static str {
    match sev {
        Some(DiagnosticSeverity::ERROR) => "error",
        Some(DiagnosticSeverity::WARNING) => "warn",
        Some(DiagnosticSeverity::INFORMATION) => "info",
        Some(DiagnosticSeverity::HINT) => "hint",
        _ => "diag",
    }
}

/// Build [`DiagnosticView`] entries from a slice of LSP `Diagnostic`s for
/// a given path.
pub fn diagnostic_views(path: &Path, diagnostics: &[Diagnostic]) -> Vec<DiagnosticView> {
    diagnostics
        .iter()
        .map(|d| DiagnosticView {
            path: path.to_path_buf(),
            line: d.range.start.line.saturating_add(1),
            column: d.range.start.character.saturating_add(1),
            severity: diagnostic_severity_tag(d.severity),
            message: d.message.clone(),
        })
        .collect()
}

/// Format a slice of [`DiagnosticView`] as one line per diagnostic. Stable
/// ordering — sort by path, then line.
pub fn format_diagnostics(views: &[DiagnosticView], workspace_root: &Path) -> String {
    if views.is_empty() {
        return "(clean)".to_owned();
    }
    let mut sorted = views.to_vec();
    sorted.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.column.cmp(&b.column))
    });
    let mut out = String::new();
    for (i, d) in sorted.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let display = relativize(&d.path, workspace_root);
        out.push_str(&format!(
            "{display}:{}:{} [{}] {}",
            d.line, d.column, d.severity, d.message
        ));
    }
    out
}

/// Filter diagnostics to those within `±buffer` lines of any line in
/// `changed_lines`. Used by writethrough so the model only sees diagnostics
/// caused by its edits, not pre-existing issues elsewhere.
pub fn filter_diagnostics_to_changed_region(
    views: Vec<DiagnosticView>,
    changed_lines: &[u32],
    buffer: u32,
) -> (Vec<DiagnosticView>, usize) {
    if changed_lines.is_empty() {
        return (Vec::new(), views.len());
    }
    let in_window = |line: u32| {
        changed_lines
            .iter()
            .any(|&c| line + buffer >= c && c + buffer >= line)
    };
    let total = views.len();
    let kept: Vec<_> = views.into_iter().filter(|v| in_window(v.line)).collect();
    let elsewhere = total - kept.len();
    (kept, elsewhere)
}

// ── WorkspaceEdit → unified diff (Phase N4) ──────────────────────────────

/// Render a [`WorkspaceEdit`] as a per-file summary the approval modal can
/// show. Each entry is `(path, edit-count, edits)` so the caller can sum or
/// expand on demand.
pub fn workspace_edit_summary(
    edit: &WorkspaceEdit,
    workspace_root: &Path,
) -> Vec<(String, usize, Vec<TextEdit>)> {
    let mut by_path: BTreeMap<String, Vec<TextEdit>> = BTreeMap::new();

    if let Some(changes) = &edit.changes {
        for (uri, edits) in changes {
            let display = uri
                .to_file_path()
                .map(|p| relativize(&p, workspace_root))
                .unwrap_or_else(|_| uri.to_string());
            by_path.entry(display).or_default().extend(edits.clone());
        }
    }
    if let Some(doc_changes) = &edit.document_changes {
        match doc_changes {
            lsp_types::DocumentChanges::Edits(edits) => {
                for ed in edits {
                    let display = ed
                        .text_document
                        .uri
                        .to_file_path()
                        .map(|p| relativize(&p, workspace_root))
                        .unwrap_or_else(|_| ed.text_document.uri.to_string());
                    let plain: Vec<TextEdit> = ed
                        .edits
                        .iter()
                        .filter_map(|e| match e {
                            lsp_types::OneOf::Left(te) => Some(te.clone()),
                            lsp_types::OneOf::Right(_) => None,
                        })
                        .collect();
                    by_path.entry(display).or_default().extend(plain);
                }
            }
            lsp_types::DocumentChanges::Operations(_) => {
                // Resource-creation/rename/delete ops are out of scope for
                // N4. The renderer will surface a summary line so the
                // model knows it skipped something.
            }
        }
    }

    by_path
        .into_iter()
        .map(|(path, edits)| (path, edits.len(), edits))
        .collect()
}

/// Format a [`WorkspaceEdit`] as a human-readable summary block:
///
///     ── Workspace edit (3 files, 12 edits) ──
///     src/lib.rs (4 edits)
///     src/app.rs (5 edits)
///     src/lsp/mod.rs (3 edits)
pub fn format_workspace_edit_summary(edit: &WorkspaceEdit, workspace_root: &Path) -> String {
    let summary = workspace_edit_summary(edit, workspace_root);
    if summary.is_empty() {
        return "(empty workspace edit — server returned nothing to apply)".to_owned();
    }
    let total_files = summary.len();
    let total_edits: usize = summary.iter().map(|(_, n, _)| n).sum();
    let mut out = format!(
        "── Workspace edit ({} {}, {} {}) ──",
        total_files,
        if total_files == 1 { "file" } else { "files" },
        total_edits,
        if total_edits == 1 { "edit" } else { "edits" }
    );
    for (path, count, _) in &summary {
        out.push('\n');
        out.push_str(&format!(
            "{path} ({} {})",
            count,
            if *count == 1 { "edit" } else { "edits" }
        ));
    }
    out
}

fn relativize(path: &Path, workspace_root: &Path) -> String {
    path.strip_prefix(workspace_root)
        .map(|rel| rel.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

/// Convert a `textDocument/hover` response into a [`HoverView`].
/// Markdown-fence trimming follows the same shape oh-my-pi uses: collapse
/// triple-backticked code fences to plain text, drop empty leading/trailing
/// blank lines, but otherwise leave the contents alone.
pub fn hover_view(hover: Hover) -> Option<HoverView> {
    let contents = hover_contents_to_text(&hover.contents);
    if contents.trim().is_empty() {
        None
    } else {
        Some(HoverView { contents })
    }
}

fn hover_contents_to_text(contents: &HoverContents) -> String {
    match contents {
        HoverContents::Scalar(scalar) => marked_string_text(scalar),
        HoverContents::Array(parts) => parts
            .iter()
            .map(marked_string_text)
            .collect::<Vec<_>>()
            .join("\n\n"),
        HoverContents::Markup(markup) => strip_code_fences(&markup.value),
    }
}

fn marked_string_text(s: &MarkedString) -> String {
    match s {
        MarkedString::String(text) => strip_code_fences(text),
        MarkedString::LanguageString(ls) => ls.value.clone(),
    }
}

/// Drop ```lang fences from a markdown blob so the model gets the inner code
/// without the rendering syntax. Conservative: leaves inline code, lists,
/// emphasis untouched.
fn strip_code_fences(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut in_fence = false;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        // Same handling either way today; the branch is kept for clarity
        // and so future fence-aware rendering (e.g. unwrapping language
        // hints) has a place to land.
        out.push_str(line);
        out.push('\n');
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{MarkupContent, MarkupKind, Position};

    /// Build a `file://` URL that round-trips cleanly on every platform.
    /// Plain `file:///x/...` URLs are valid on Unix but fail
    /// `Url::to_file_path()` on Windows because they lack a drive letter,
    /// so we go through `Url::from_file_path` with the platform-native
    /// temp dir to get a path the OS actually believes in.
    fn fixture_uri() -> Url {
        let mut path = std::env::temp_dir();
        path.push("artui-render-test");
        path.push("lib.rs");
        Url::from_file_path(&path)
            .expect("temp_dir should yield a path that round-trips through file://")
    }

    fn loc(uri: Url, line: u32, col: u32) -> Location {
        Location {
            uri,
            range: Range {
                start: Position {
                    line,
                    character: col,
                },
                end: Position {
                    line,
                    character: col + 1,
                },
            },
        }
    }

    #[test]
    fn scalar_definition_renders_one_hit() {
        let resp = GotoDefinitionResponse::Scalar(loc(fixture_uri(), 5, 4));
        let views = locations_from_response(resp);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].line, 6);
        assert_eq!(views[0].column, 5);
    }

    #[test]
    fn array_definition_caps_at_max_hits() {
        let many: Vec<_> = (0..20).map(|i| loc(fixture_uri(), i, 0)).collect();
        let resp = GotoDefinitionResponse::Array(many);
        let views = locations_from_response(resp);
        assert_eq!(views.len(), MAX_LOCATION_HITS);
    }

    #[test]
    fn link_definition_uses_target_selection_range() {
        let link = LocationLink {
            origin_selection_range: None,
            target_uri: fixture_uri(),
            target_range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 9,
                    character: 0,
                },
            },
            target_selection_range: Range {
                start: Position {
                    line: 7,
                    character: 3,
                },
                end: Position {
                    line: 7,
                    character: 9,
                },
            },
        };
        let resp = GotoDefinitionResponse::Link(vec![link]);
        let views = locations_from_response(resp);
        assert_eq!(views[0].line, 8);
        assert_eq!(views[0].column, 4);
    }

    #[test]
    fn format_locations_relativizes_to_workspace() {
        let mut workspace = std::env::temp_dir();
        workspace.push("artui-render-fmt");
        let views = vec![LocationView {
            path: workspace.join("src").join("lib.rs"),
            line: 12,
            column: 8,
            preview: None,
        }];
        let out = format_locations(&views, &workspace);
        // OS-native separator; assert containment so we don't bake `/` vs `\\` in.
        assert!(out.contains("lib.rs:12:8"), "got: {out}");
        assert!(out.contains("src"), "got: {out}");
    }

    #[test]
    fn format_locations_handles_empty() {
        let workspace = std::env::temp_dir();
        let out = format_locations(&[], &workspace);
        assert_eq!(out, "no definition found");
    }

    #[test]
    fn hover_view_strips_markdown_fences() {
        let hover = Hover {
            range: None,
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: "```rust\nfn foo() -> i32\n```\n\nReturns the answer.".to_owned(),
            }),
        };
        let view = hover_view(hover).unwrap();
        assert!(view.contents.contains("fn foo() -> i32"));
        assert!(view.contents.contains("Returns the answer."));
        assert!(!view.contents.contains("```"));
    }

    #[test]
    fn hover_view_returns_none_for_blank_contents() {
        let hover = Hover {
            range: None,
            contents: HoverContents::Scalar(MarkedString::String("".to_owned())),
        };
        assert!(hover_view(hover).is_none());
    }
}
