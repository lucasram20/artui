//! [`LspManager`] — workspace-wide cache of [`LspClient`] instances.
//!
//! - Lazy spawn-on-demand via [`Self::for_path`]. First call for a
//!   `(server_id, root)` pair builds the client; subsequent calls hit the
//!   cache.
//! - [`Self::warmup`] enumerates root markers under the workspace and
//!   pre-spawns clients for any server with at least one matching file.
//! - [`Self::shutdown`] iterates all clients and drops them, sending
//!   `shutdown` + `exit` per client.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{info, warn};

use crate::app::AppEvent;

use super::client::LspClient;
use super::registry::ServerRegistry;
use super::types::ServerSpec;

const WARMUP_WALK_DEPTH: usize = 6;
const WARMUP_FILE_BUDGET: usize = 200;

/// Cache key for an `LspClient`: `(server_id, workspace_root)`.
type ClientKey = (String, PathBuf);
/// One cached client. `Arc<Mutex<…>>` so concurrent dispatches share state.
type ClientHandle = Arc<Mutex<LspClient>>;
type ClientCache = HashMap<ClientKey, ClientHandle>;

/// Summary of a warmup pass — useful for tests and `lsp` `status` action.
#[derive(Debug, Default, Clone)]
pub struct WarmupReport {
    /// `(server_id, root)` pairs that came up cleanly.
    pub started: Vec<(String, PathBuf)>,
    /// `(server_id, root, error)` for servers that failed to spawn.
    /// Errors are non-fatal — the manager still returns a usable handle.
    pub failed: Vec<(String, PathBuf, String)>,
}

pub struct LspManager {
    registry: ServerRegistry,
    clients: RwLock<ClientCache>,
    events: mpsc::Sender<AppEvent>,
}

impl LspManager {
    pub fn new(registry: ServerRegistry, events: mpsc::Sender<AppEvent>) -> Self {
        Self {
            registry,
            clients: RwLock::new(HashMap::new()),
            events,
        }
    }

    pub fn registry(&self) -> &ServerRegistry {
        &self.registry
    }

    /// Spawn-or-fetch the client serving `path`.
    ///
    /// Errors out when the file's extension isn't claimed by any registered
    /// server, or when the server binary isn't on `$PATH`. The error
    /// message is shaped for the model — callers can surface it directly.
    pub async fn for_path(
        self: &Arc<Self>,
        path: &Path,
        cwd_fallback: &Path,
    ) -> Result<ClientHandle> {
        let (server_id, root) = self.registry.resolve(path, cwd_fallback).ok_or_else(|| {
            anyhow::anyhow!(
                "no language server registered for `{}`",
                path.extension()
                    .and_then(|s| s.to_str())
                    .map(|s| format!(".{s}"))
                    .unwrap_or_else(|| path.display().to_string())
            )
        })?;
        self.get_or_spawn(server_id, root).await
    }

    async fn get_or_spawn(
        self: &Arc<Self>,
        server_id: String,
        root: PathBuf,
    ) -> Result<ClientHandle> {
        let key = (server_id.clone(), root.clone());
        {
            let clients = self.clients.read().await;
            if let Some(existing) = clients.get(&key) {
                return Ok(Arc::clone(existing));
            }
        }
        let spec = self.registry.get(&server_id).cloned().with_context(|| {
            format!("server `{server_id}` was resolved but is missing from registry")
        })?;
        let client = LspClient::spawn(&server_id, &spec, &root, self.events.clone())
            .await
            .with_context(|| format!("failed to start `{server_id}`"))?;
        let arc = Arc::new(Mutex::new(client));
        let mut clients = self.clients.write().await;
        // Recheck — another task may have raced.
        if let Some(existing) = clients.get(&key) {
            return Ok(Arc::clone(existing));
        }
        clients.insert(key, Arc::clone(&arc));
        Ok(arc)
    }

    /// Walk `cwd` looking for project markers and pre-spawn matching clients.
    ///
    /// Cancel-safe: stop polling as soon as we've collected enough samples.
    /// Failures per server are reported via [`WarmupReport`] but never block
    /// the agent loop — the user can still use LSP for any path the
    /// registry resolves.
    pub async fn warmup(self: &Arc<Self>, cwd: &Path) -> WarmupReport {
        let mut report = WarmupReport::default();
        let candidates = scan_workspace(&self.registry, cwd, WARMUP_FILE_BUDGET);

        for (server_id, root) in candidates {
            match self.get_or_spawn(server_id.clone(), root.clone()).await {
                Ok(_) => {
                    info!(target: "lsp", "warmed up {server_id} at {}", root.display());
                    report.started.push((server_id, root));
                }
                Err(error) => {
                    warn!(target: "lsp", "warmup failed for {server_id}: {error}");
                    report.failed.push((server_id, root, format!("{error:#}")));
                }
            }
        }
        report
    }

    /// Snapshot status info for the `lsp` `status` action.
    pub async fn status_snapshot(&self) -> Vec<ManagerClientSnapshot> {
        let clients = self.clients.read().await;
        let mut out = Vec::with_capacity(clients.len());
        for ((id, root), arc) in clients.iter() {
            let client = arc.lock().await;
            out.push(ManagerClientSnapshot {
                server_id: id.clone(),
                root: root.clone(),
                capabilities_initialized: client.capabilities().await.is_some(),
            });
        }
        out
    }

    /// Drop all clients, sending shutdown + exit to each.
    pub async fn shutdown(&self) {
        let mut clients = self.clients.write().await;
        let drained: Vec<_> = clients.drain().map(|(_, arc)| arc).collect();
        drop(clients);
        for arc in drained {
            let mut client = arc.lock().await;
            client.shutdown().await;
        }
    }
}

/// Snapshot returned by [`LspManager::status_snapshot`] — used by the `lsp`
/// `status` action to render a list of running servers.
#[derive(Debug, Clone)]
pub struct ManagerClientSnapshot {
    pub server_id: String,
    pub root: PathBuf,
    pub capabilities_initialized: bool,
}

/// Walk `cwd` (bounded depth) and produce a deduplicated list of
/// `(server_id, root)` pairs to warm up. The walker respects `.gitignore`
/// via the `ignore` crate (already a dep) so we don't pull in `target/`,
/// `node_modules/`, or `.git/`.
fn scan_workspace(
    registry: &ServerRegistry,
    cwd: &Path,
    max_files: usize,
) -> Vec<(String, PathBuf)> {
    let mut seen: HashMap<(String, PathBuf), ()> = HashMap::new();
    let walker = ignore::WalkBuilder::new(cwd)
        .max_depth(Some(WARMUP_WALK_DEPTH))
        .standard_filters(true)
        .build();
    let mut visited = 0usize;
    for entry in walker.flatten() {
        if visited >= max_files {
            break;
        }
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        visited += 1;
        if let Some((server_id, root)) = registry.resolve(path, cwd) {
            seen.entry((server_id, root)).or_insert(());
        }
    }
    let mut out: Vec<_> = seen.into_keys().collect();
    out.sort();
    out
}

/// Convenience: lookup `ServerSpec` for the given id, returning a clone the
/// caller can move into `tokio::spawn`.
pub fn cloned_spec(registry: &ServerRegistry, id: &str) -> Option<ServerSpec> {
    registry.get(id).cloned()
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
command = "rust-analyzer-zzz-not-found"
file_types = ["rs"]
root_markers = ["Cargo.toml"]

[server.gopls]
command = "gopls-zzz-not-found"
file_types = ["go"]
root_markers = ["go.mod"]
"#,
        )
        .unwrap()
    }

    fn make_manager(registry: ServerRegistry) -> Arc<LspManager> {
        let (tx, _rx) = mpsc::channel(8);
        Arc::new(LspManager::new(registry, tx))
    }

    #[tokio::test]
    async fn for_path_returns_clear_error_for_unknown_extension() {
        let manager = make_manager(fixture_registry());
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("note.zig");
        fs::write(&path, "// zig").unwrap();
        let err = manager.for_path(&path, dir.path()).await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("no language server"), "got: {msg}");
    }

    #[tokio::test]
    async fn for_path_propagates_missing_executable_error() {
        let manager = make_manager(fixture_registry());
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        let path = dir.path().join("main.rs");
        fs::write(&path, "fn main() {}").unwrap();
        let err = manager.for_path(&path, dir.path()).await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("not found"),
            "expected missing-binary error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn warmup_reports_failures_without_blocking() {
        let manager = make_manager(fixture_registry());
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        let report = manager.warmup(dir.path()).await;
        assert!(report.started.is_empty(), "expected no successful spawns");
        assert!(
            !report.failed.is_empty(),
            "expected failure for missing binary"
        );
        assert_eq!(report.failed[0].0, "rust-analyzer");
    }

    #[tokio::test]
    async fn shutdown_clears_cache() {
        let manager = make_manager(fixture_registry());
        manager.shutdown().await;
        assert!(manager.clients.read().await.is_empty());
    }

    #[tokio::test]
    async fn status_snapshot_reports_empty_when_idle() {
        let manager = make_manager(fixture_registry());
        let snap = manager.status_snapshot().await;
        assert!(snap.is_empty());
    }
}
