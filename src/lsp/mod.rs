//! Language Server Protocol support.
//!
//! Wires `async-lsp`-backed clients into artui's tool loop so the agent can
//! ask "where's this defined", "what's the type of X", and (in later phases)
//! "rename this symbol everywhere" without re-implementing those queries
//! with grep + Tree-sitter.
//!
//! ## Module layout
//!
//! - [`registry`] — parses [`defaults.toml`](defaults) and the user overlay
//!   at `~/.config/artui/lsp.toml`. Resolves a workspace path to
//!   `(server_id, root)` via extension lookup + root-marker walk.
//! - [`client`] — one [`LspClient`] per `(server, root)`. Owns the child
//!   process, the `async_lsp::ServerSocket`, and the cached
//!   `ServerCapabilities` / diagnostics map.
//! - [`manager`] — [`LspManager`]: workspace-wide cache keyed by
//!   `(server_id, root)`. Lazy spawn-on-demand. `warmup(cwd)` runs as a
//!   background task at startup. Graceful shutdown on `Drop`.
//! - [`render`] — turns `lsp_types::Location` / `lsp_types::Hover` into
//!   strings the model can act on.
//! - [`types`] — shared data types: [`ServerSpec`], [`LspAction`],
//!   [`LocationView`], [`HoverView`].
//!
//! ## Vendored registry
//!
//! `defaults.toml` is **vendored from helix-editor's `languages.toml`**
//! ([MPL-2.0]) and ported to artui's flat `[server.<id>]` schema by
//! `scripts/sync-helix-lsp.py`. License compliance: see `src/lsp/NOTICE`.
//!
//! [MPL-2.0]: https://github.com/helix-editor/helix/blob/master/LICENSE

pub mod client;
pub mod edits;
pub mod manager;
pub mod registry;
pub mod render;
pub mod types;
pub mod writethrough;

pub use client::LspClient;
pub use edits::{apply_workspace_edit, ApplyReport};
pub use manager::{LspManager, WarmupReport};
pub use registry::ServerRegistry;
pub use types::{DiagnosticView, HoverView, LocationView, LspAction, ServerSpec, SymbolView};
pub use writethrough::{after_edit, EditedFile, WritethroughOutcome};
