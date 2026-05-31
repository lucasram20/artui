//! Shared data types for the LSP module.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

/// One language-server entry in the registry — flat record describing how
/// to spawn the server and which files it serves.
///
/// Vendored entries come from `defaults.toml` (helix-editor `languages.toml`
/// ported by `scripts/sync-helix-lsp.py`); user-supplied entries live in
/// `~/.config/artui/lsp.toml` and merge over the defaults.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ServerSpec {
    /// Executable name. Resolved against `$PATH` at spawn time.
    pub command: String,
    /// CLI arguments passed verbatim to the spawn.
    #[serde(default)]
    pub args: Vec<String>,
    /// File extensions this server claims (no leading dot, e.g. `["rs"]`).
    #[serde(default)]
    pub file_types: Vec<String>,
    /// Workspace root markers. The registry walks up from a file looking for
    /// any of these — first hit wins. Falls back to the workspace cwd if
    /// nothing matches.
    #[serde(default)]
    pub root_markers: Vec<String>,
    /// `initializationOptions` passed verbatim into the LSP `initialize`
    /// request. Stored as a JSON string so user-supplied tables of arbitrary
    /// shape round-trip cleanly through TOML — `defaults.toml` ships these
    /// as `init_options_json = "{...}"` because helix's nested config tables
    /// don't map cleanly to a single TOML inline expression.
    ///
    /// User-supplied entries in `~/.config/artui/lsp.toml` may use either
    /// form: `init_options_json = "..."` or a regular `[server.x.init_options]`
    /// nested table. The latter is parsed via [`Self::init_options_value`].
    #[serde(default, rename = "init_options_json")]
    pub init_options_json: Option<String>,
    /// Alternate spelling for user configs that prefer a nested TOML table
    /// over a JSON string. Both fields can be present; `init_options_json`
    /// wins if it is.
    #[serde(default)]
    pub init_options: Option<toml::Value>,
    /// Free-form environment overlay applied to the spawned process. Useful
    /// for `RA_LOG`, `GOPLS_LOG`, etc.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

impl ServerSpec {
    /// Returns true if `extension` matches one of this server's `file_types`.
    /// Comparison is case-insensitive and ignores any leading dot on
    /// `extension`.
    pub fn handles_extension(&self, extension: &str) -> bool {
        let ext = extension.trim_start_matches('.').to_ascii_lowercase();
        self.file_types
            .iter()
            .any(|ft| ft.trim_start_matches('.').eq_ignore_ascii_case(&ext))
    }

    /// Resolve the `initializationOptions` payload to a `serde_json::Value`.
    /// `init_options_json` (preferred form for the bundled defaults) wins
    /// over the nested-TOML `init_options` form.
    ///
    /// Returns `None` if neither field is set or if the JSON string fails
    /// to parse — callers should treat that as "no init options" rather
    /// than an error, matching how clients with broken configs fall back
    /// to upstream LSP defaults.
    pub fn init_options_value(&self) -> Option<serde_json::Value> {
        if let Some(raw) = &self.init_options_json {
            return serde_json::from_str(raw).ok();
        }
        if let Some(value) = &self.init_options {
            // Round-trip the TOML value through serde_json so we don't
            // hand `lsp_types` a foreign Value implementation.
            return serde_json::to_value(value).ok();
        }
        None
    }
}

/// Operations the `lsp` tool dispatches.
///
/// Phase N1: [`Self::Definition`], [`Self::Hover`], [`Self::Status`].
/// Phase N2: [`Self::References`], [`Self::Implementation`],
///   [`Self::TypeDefinition`], [`Self::DocumentSymbols`],
///   [`Self::WorkspaceSymbols`], [`Self::Diagnostics`].
/// Phase N4: [`Self::Rename`], [`Self::CodeActions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspAction {
    Definition,
    Hover,
    Status,
    References,
    Implementation,
    TypeDefinition,
    DocumentSymbols,
    WorkspaceSymbols,
    Diagnostics,
    Rename,
    CodeActions,
}

impl LspAction {
    /// Parse an action string from the tool args. Unknown values return
    /// `None` so the caller can produce a clear error.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "definition" => Some(Self::Definition),
            "hover" => Some(Self::Hover),
            "status" => Some(Self::Status),
            "references" => Some(Self::References),
            "implementation" => Some(Self::Implementation),
            "type_definition" => Some(Self::TypeDefinition),
            "document_symbols" => Some(Self::DocumentSymbols),
            "workspace_symbols" => Some(Self::WorkspaceSymbols),
            "diagnostics" => Some(Self::Diagnostics),
            "rename" => Some(Self::Rename),
            "code_actions" => Some(Self::CodeActions),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Definition => "definition",
            Self::Hover => "hover",
            Self::Status => "status",
            Self::References => "references",
            Self::Implementation => "implementation",
            Self::TypeDefinition => "type_definition",
            Self::DocumentSymbols => "document_symbols",
            Self::WorkspaceSymbols => "workspace_symbols",
            Self::Diagnostics => "diagnostics",
            Self::Rename => "rename",
            Self::CodeActions => "code_actions",
        }
    }

    /// Mutating ops route through the approval engine. Read-only ops bypass
    /// it (indistinguishable from `read_file`).
    pub fn is_mutating(self) -> bool {
        matches!(self, Self::Rename | Self::CodeActions)
    }
}

/// Rendered `textDocument/definition` hit, ready for the model.
///
/// The renderer in [`super::render`] formats one line per hit:
/// `path:line:col` (1-based, matching what humans paste into a terminal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationView {
    pub path: PathBuf,
    /// 1-based line number.
    pub line: u32,
    /// 1-based column number.
    pub column: u32,
    /// Optional preview line — populated for `references` so the model
    /// sees the surrounding context. Empty for `definition` hits.
    pub preview: Option<String>,
}

/// Rendered `textDocument/hover` payload — markdown stripped to plain text
/// by default so the model sees the same string a human would.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverView {
    pub contents: String,
}

/// One symbol from `textDocument/documentSymbol` — flattened with depth so
/// the renderer can emit a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolView {
    /// Display name, e.g. `LspManager` or `for_path`.
    pub name: String,
    /// LSP `SymbolKind` rendered as a short tag (`fn`, `struct`, `mod`, …).
    pub kind: &'static str,
    /// Indent depth (0 = top-level).
    pub depth: u32,
    /// 1-based line of the symbol's selection range.
    pub line: u32,
    /// 1-based column of the symbol's selection range.
    pub column: u32,
}

/// One diagnostic, post-render. Severity tag + position + message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticView {
    pub path: PathBuf,
    pub line: u32,
    pub column: u32,
    pub severity: &'static str,
    pub message: String,
}
