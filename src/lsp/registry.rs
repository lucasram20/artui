//! [`ServerRegistry`] — parses the bundled defaults plus the user overlay
//! and resolves a workspace path to `(server_id, workspace_root)`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use super::types::ServerSpec;

/// Bundled server definitions, vendored from helix-editor's `languages.toml`
/// (MPL-2.0). See `src/lsp/NOTICE` for attribution and the script at
/// `scripts/sync-helix-lsp.py` for regeneration.
const DEFAULTS_TOML: &str = include_str!("defaults.toml");

#[derive(Debug, Default, Deserialize)]
struct RegistryFile {
    /// `[server.<id>]` table. Same key namespace as helix.
    #[serde(default)]
    server: BTreeMap<String, ServerSpec>,
    /// User-config legacy spelling — `[servers.<id>]`. Accepted as a fallback
    /// so users who copy/paste from helix's docs don't have to know about
    /// our singular spelling.
    #[serde(default)]
    servers: BTreeMap<String, ServerSpec>,
}

impl RegistryFile {
    fn into_specs(self) -> BTreeMap<String, ServerSpec> {
        let mut out = self.server;
        for (k, v) in self.servers {
            out.entry(k).or_insert(v);
        }
        out
    }
}

/// Read-only registry of language servers.
#[derive(Debug, Clone)]
pub struct ServerRegistry {
    servers: BTreeMap<String, ServerSpec>,
}

impl ServerRegistry {
    /// Build the registry from the embedded defaults plus the user overlay
    /// at `~/.config/artui/lsp.toml` (when present).
    pub fn load() -> Result<Self> {
        let mut registry = Self::from_defaults()?;
        if let Some(overlay_path) = user_overlay_path() {
            registry.merge_user_overlay(&overlay_path)?;
        }
        Ok(registry)
    }

    /// Build the registry from just the bundled defaults.
    pub fn from_defaults() -> Result<Self> {
        let parsed: RegistryFile = toml::from_str(DEFAULTS_TOML)
            .context("failed to parse bundled src/lsp/defaults.toml")?;
        Ok(Self {
            servers: parsed.into_specs(),
        })
    }

    /// Build a registry from a TOML string. Test-only entry point — production
    /// code should use [`Self::load`].
    pub fn from_toml_str(input: &str) -> Result<Self> {
        let parsed: RegistryFile =
            toml::from_str(input).context("failed to parse registry TOML")?;
        Ok(Self {
            servers: parsed.into_specs(),
        })
    }

    /// Merge a user overlay file. Missing files are not errors — users
    /// without an `lsp.toml` get the bundled defaults verbatim.
    pub fn merge_user_overlay(&mut self, path: &Path) -> Result<()> {
        let raw = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                return Err(anyhow::Error::new(e)
                    .context(format!("failed to read user overlay: {}", path.display())))
            }
        };
        let parsed: RegistryFile = toml::from_str(&raw)
            .with_context(|| format!("failed to parse user overlay: {}", path.display()))?;
        for (id, spec) in parsed.into_specs() {
            self.servers.insert(id, spec);
        }
        Ok(())
    }

    /// Get a spec by id (e.g. `"rust-analyzer"`).
    pub fn get(&self, id: &str) -> Option<&ServerSpec> {
        self.servers.get(id)
    }

    /// Iterate over all registered servers.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &ServerSpec)> {
        self.servers.iter()
    }

    /// Number of registered servers.
    pub fn len(&self) -> usize {
        self.servers.len()
    }

    /// Returns true when the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    /// Resolve a file path to a `(server_id, workspace_root)` pair.
    ///
    /// 1. Look up servers that handle the file's extension.
    /// 2. For each candidate, walk up from the file looking for any
    ///    `root_markers` entry. First hit wins, deepest match preferred
    ///    when several candidates apply.
    /// 3. If no markers match, fall back to `cwd_fallback` (typically the
    ///    workspace root) so we still attach a sensible client.
    /// 4. Returns `None` only when no server claims this extension.
    pub fn resolve(&self, path: &Path, cwd_fallback: &Path) -> Option<(String, PathBuf)> {
        let extension = path.extension().and_then(|s| s.to_str())?;

        let candidates: Vec<(&String, &ServerSpec)> = self
            .servers
            .iter()
            .filter(|(_, spec)| spec.handles_extension(extension))
            .collect();
        if candidates.is_empty() {
            return None;
        }

        let mut best: Option<(String, PathBuf, usize)> = None;

        for (id, spec) in &candidates {
            if let Some((root, depth)) = walk_up_for_marker(path, &spec.root_markers) {
                let depth_score = root.components().count();
                let entry = (id.to_string(), root, depth_score * 1000 + depth);
                best = match best {
                    Some(prev) if prev.2 >= entry.2 => Some(prev),
                    _ => Some(entry),
                };
            }
        }

        if let Some((id, root, _)) = best {
            return Some((id, root));
        }

        // No markers matched — fall back to cwd_fallback for the first
        // candidate. The model still gets LSP for files in workspaces
        // without explicit project markers (scratch dirs, single-file
        // edits, etc.).
        let (id, _) = candidates.first()?;
        Some(((*id).clone(), cwd_fallback.to_path_buf()))
    }
}

fn walk_up_for_marker(path: &Path, markers: &[String]) -> Option<(PathBuf, usize)> {
    if markers.is_empty() {
        return None;
    }
    let start = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    let mut depth = 0;
    let mut current = Some(start.as_path());
    while let Some(dir) = current {
        for marker in markers {
            let candidate = dir.join(marker);
            if candidate.exists() {
                return Some((dir.to_path_buf(), depth));
            }
        }
        current = dir.parent();
        depth += 1;
    }
    None
}

/// Resolve `~/.config/artui/lsp.toml` (or the platform equivalent) using the
/// same `directories` crate the rest of artui's config loader leans on.
fn user_overlay_path() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "artui")?;
    let mut path = dirs.config_dir().to_path_buf();
    path.push("lsp.toml");
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn fixture_registry() -> ServerRegistry {
        ServerRegistry::from_toml_str(
            r#"
[server.rust-analyzer]
command = "rust-analyzer"
file_types = ["rs"]
root_markers = ["Cargo.toml", "rust-project.json"]

[server.gopls]
command = "gopls"
file_types = ["go"]
root_markers = ["go.mod"]

[server.typescript-language-server]
command = "typescript-language-server"
args = ["--stdio"]
file_types = ["ts", "tsx", "js"]
root_markers = ["package.json", "tsconfig.json"]
"#,
        )
        .unwrap()
    }

    #[test]
    fn parses_defaults_toml() {
        let registry = ServerRegistry::from_defaults().unwrap();
        assert!(
            registry.len() >= 5,
            "expected ≥5 default servers, got {}",
            registry.len()
        );
        for required in [
            "rust-analyzer",
            "gopls",
            "pyright",
            "typescript-language-server",
            "clangd",
        ] {
            assert!(
                registry.get(required).is_some(),
                "default registry missing `{required}`"
            );
        }
    }

    #[test]
    fn handles_extension_is_case_insensitive() {
        let registry = fixture_registry();
        let spec = registry.get("rust-analyzer").unwrap();
        assert!(spec.handles_extension("rs"));
        assert!(spec.handles_extension("RS"));
        assert!(spec.handles_extension(".rs"));
        assert!(!spec.handles_extension("py"));
    }

    #[test]
    fn resolve_finds_server_via_extension_and_marker() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        let file = dir.path().join("src/main.rs");
        fs::write(&file, "fn main() {}").unwrap();

        let registry = fixture_registry();
        let (id, root) = registry
            .resolve(&file, dir.path())
            .expect("resolve must succeed");
        assert_eq!(id, "rust-analyzer");
        assert_eq!(root, dir.path());
    }

    #[test]
    fn resolve_walks_up_to_find_marker() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"deep\"\n").unwrap();
        let nested = dir.path().join("a/b/c");
        fs::create_dir_all(&nested).unwrap();
        let file = nested.join("buried.rs");
        fs::write(&file, "fn x() {}").unwrap();

        let registry = fixture_registry();
        let (id, root) = registry.resolve(&file, dir.path()).unwrap();
        assert_eq!(id, "rust-analyzer");
        assert_eq!(root, dir.path());
    }

    #[test]
    fn resolve_falls_back_to_cwd_when_no_marker_present() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("scratch.rs");
        fs::write(&file, "fn x() {}").unwrap();

        let registry = fixture_registry();
        let (id, root) = registry.resolve(&file, dir.path()).unwrap();
        assert_eq!(id, "rust-analyzer");
        assert_eq!(root, dir.path());
    }

    #[test]
    fn resolve_returns_none_for_unsupported_extension() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("note.zig");
        fs::write(&file, "// zig").unwrap();

        let registry = fixture_registry();
        assert!(registry.resolve(&file, dir.path()).is_none());
    }

    #[test]
    fn user_overlay_overrides_defaults() {
        let dir = TempDir::new().unwrap();
        let overlay = dir.path().join("lsp.toml");
        fs::write(
            &overlay,
            r#"
[server.rust-analyzer]
command = "ra-fork"
file_types = ["rs"]
root_markers = ["Cargo.toml"]
init_options = { checkOnSave = { command = "check" } }
"#,
        )
        .unwrap();

        let mut registry = fixture_registry();
        registry.merge_user_overlay(&overlay).unwrap();
        assert_eq!(registry.get("rust-analyzer").unwrap().command, "ra-fork");
    }

    #[test]
    fn user_overlay_accepts_servers_legacy_spelling() {
        let mut registry = fixture_registry();
        let dir = TempDir::new().unwrap();
        let overlay = dir.path().join("lsp.toml");
        fs::write(
            &overlay,
            r#"
[servers.deno]
command = "deno"
args = ["lsp"]
file_types = ["ts"]
root_markers = ["deno.json"]
"#,
        )
        .unwrap();
        registry.merge_user_overlay(&overlay).unwrap();
        assert!(registry.get("deno").is_some());
    }

    #[test]
    fn missing_overlay_is_not_an_error() {
        let mut registry = fixture_registry();
        let dir = TempDir::new().unwrap();
        let absent = dir.path().join("does-not-exist.toml");
        registry.merge_user_overlay(&absent).unwrap();
        assert!(registry.get("rust-analyzer").is_some());
    }

    #[test]
    fn unknown_server_returns_none() {
        let registry = fixture_registry();
        assert!(registry.get("nonexistent").is_none());
    }
}
