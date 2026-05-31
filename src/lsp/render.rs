//! Rendering helpers — turn `lsp_types` payloads into strings the model
//! can act on without further parsing.

use std::path::Path;

use lsp_types::{
    GotoDefinitionResponse, Hover, HoverContents, Location, LocationLink, MarkedString, Range, Url,
};

use super::types::{HoverView, LocationView};

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
    })
}

/// Format a list of [`LocationView`] entries as one line per hit, paths made
/// relative to `workspace_root` when possible.
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

    fn loc(uri: &str, line: u32, col: u32) -> Location {
        Location {
            uri: Url::parse(uri).unwrap(),
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
        let resp = GotoDefinitionResponse::Scalar(loc("file:///x/src/lib.rs", 5, 4));
        let views = locations_from_response(resp);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].line, 6);
        assert_eq!(views[0].column, 5);
    }

    #[test]
    fn array_definition_caps_at_max_hits() {
        let many: Vec<_> = (0..20).map(|i| loc("file:///x/lib.rs", i, 0)).collect();
        let resp = GotoDefinitionResponse::Array(many);
        let views = locations_from_response(resp);
        assert_eq!(views.len(), MAX_LOCATION_HITS);
    }

    #[test]
    fn link_definition_uses_target_selection_range() {
        let link = LocationLink {
            origin_selection_range: None,
            target_uri: Url::parse("file:///x/lib.rs").unwrap(),
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
        let workspace = std::path::PathBuf::from("/work/proj");
        let views = vec![LocationView {
            path: workspace.join("src/lib.rs"),
            line: 12,
            column: 8,
        }];
        let out = format_locations(&views, &workspace);
        assert_eq!(out, "src/lib.rs:12:8");
    }

    #[test]
    fn format_locations_handles_empty() {
        let workspace = std::path::PathBuf::from("/work/proj");
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
