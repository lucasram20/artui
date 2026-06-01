//! Workspace-level snapshots: capture/restore the working tree so a single
//! command rewinds after risky agent operations. Workspace-keyed (by a hash
//! of the canonical workspace path) and independent of the session store.
//!
//! Two backends: `git` (default in a git repo — captures the full working
//! tree including untracked files via a throwaway index + `git write-tree`)
//! and `tar` (a `tar.zst` archive fallback for non-git workspaces).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::schema::SnapshotsConfig;

mod git_backend;
mod tar_backend;

/// `"snap_<ULID>"` identifier for a single snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotId(pub String);

impl SnapshotId {
    fn new() -> Self {
        SnapshotId(format!("snap_{}", ulid::Ulid::new()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Which capture mechanism produced a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Git,
    Tar,
}

/// Why a snapshot was taken (shown in `/snapshot list`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    PrePatch,
    PreShell,
    PerTurn,
    Manual,
}

impl Reason {
    pub fn label(self) -> &'static str {
        match self {
            Reason::PrePatch => "pre_patch",
            Reason::PreShell => "pre_shell",
            Reason::PerTurn => "per_turn",
            Reason::Manual => "manual",
        }
    }
}

/// Which auto-snapshots are enabled. Cloned into the agent loop config.
#[derive(Debug, Clone, Copy)]
pub struct SnapshotPolicy {
    pub auto_pre_patch: bool,
    pub auto_pre_shell: bool,
    pub auto_per_turn: bool,
}

impl SnapshotPolicy {
    pub fn from_config(cfg: &SnapshotsConfig) -> Self {
        Self {
            auto_pre_patch: cfg.auto_pre_patch,
            auto_pre_shell: cfg.auto_pre_shell,
            auto_per_turn: cfg.auto_per_turn,
        }
    }
}

/// One persisted snapshot record (one entry in `index.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub id: SnapshotId,
    pub created_at: String,
    pub backend: Backend,
    pub reason: Reason,
    /// git backend: the captured tree sha.
    pub git_tree: Option<String>,
    /// git backend: HEAD sha at capture time (None in a repo with no commits).
    pub git_head: Option<String>,
    /// tar backend: path to the `.tar.zst` archive.
    pub tar_path: Option<PathBuf>,
    /// The shell command that triggered this snapshot, if any.
    pub command: Option<String>,
}

/// Manages snapshots for a single workspace.
pub struct SnapshotManager {
    workspace_root: PathBuf,
    snapshot_root: PathBuf,
    backend: Backend,
    retain: usize,
    max_tar_mb: u64,
}

impl SnapshotManager {
    /// Resolve the snapshot dir for this workspace, detect the backend, and
    /// load the existing index. Returns `Ok(None)` when snapshots are
    /// disabled in config so callers can skip wiring entirely.
    pub fn for_workspace(workspace_root: &Path, cfg: &SnapshotsConfig) -> Result<Option<Self>> {
        if !cfg.enabled {
            return Ok(None);
        }
        let canonical = workspace_root
            .canonicalize()
            .unwrap_or_else(|_| workspace_root.to_path_buf());
        let hash = workspace_hash(&canonical);
        let data_dir = directories::ProjectDirs::from("", "", "artui")
            .context("cannot determine data directory")?
            .data_local_dir()
            .to_path_buf();
        let snapshot_root = data_dir.join("snapshots").join(hash);
        fs::create_dir_all(&snapshot_root).context("failed to create snapshot directory")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&snapshot_root, fs::Permissions::from_mode(0o700));
        }
        let backend = if git_backend::is_git_workspace(&canonical) {
            Backend::Git
        } else {
            Backend::Tar
        };
        Ok(Some(Self {
            workspace_root: canonical,
            snapshot_root,
            backend,
            retain: cfg.retain,
            max_tar_mb: cfg.max_tar_mb,
        }))
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }

    fn index_path(&self) -> PathBuf {
        self.snapshot_root.join("index.json")
    }

    fn load_index(&self) -> Vec<SnapshotMeta> {
        match fs::read_to_string(self.index_path()) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    fn save_index(&self, index: &[SnapshotMeta]) -> Result<()> {
        let json = serde_json::to_string_pretty(index).context("serialize snapshot index")?;
        fs::write(self.index_path(), json).context("write snapshot index")?;
        Ok(())
    }

    /// Capture the workspace. Returns the new id, or `Ok(None)` when the
    /// snapshot was skipped (e.g. tar size guard).
    pub fn take(&self, reason: Reason, command: Option<String>) -> Result<Option<SnapshotId>> {
        let id = SnapshotId::new();
        let mut meta = SnapshotMeta {
            id: id.clone(),
            created_at: now_iso(),
            backend: self.backend,
            reason,
            git_tree: None,
            git_head: None,
            tar_path: None,
            command,
        };
        match self.backend {
            Backend::Git => {
                let (tree, head) = git_backend::take(&self.workspace_root)?;
                meta.git_tree = Some(tree);
                meta.git_head = head;
            }
            Backend::Tar => {
                let tar_path = self.snapshot_root.join(format!("{}.tar.zst", id.as_str()));
                match tar_backend::take(&self.workspace_root, &tar_path, self.max_tar_mb)? {
                    Some(()) => meta.tar_path = Some(tar_path),
                    None => return Ok(None), // size guard tripped
                }
            }
        }
        let mut index = self.load_index();
        index.push(meta);
        self.save_index(&index)?;
        self.prune()?;
        Ok(Some(id))
    }

    /// Rewind the workspace to the named snapshot.
    pub fn restore(&self, id: &SnapshotId) -> Result<()> {
        let index = self.load_index();
        let meta = index
            .iter()
            .find(|m| &m.id == id)
            .with_context(|| format!("unknown snapshot: {id}"))?;
        match meta.backend {
            Backend::Git => {
                let tree = meta
                    .git_tree
                    .as_deref()
                    .context("git snapshot missing tree sha")?;
                git_backend::restore(&self.workspace_root, tree)
            }
            Backend::Tar => {
                let path = meta
                    .tar_path
                    .as_ref()
                    .context("tar snapshot missing archive path")?;
                tar_backend::restore(&self.workspace_root, path)
            }
        }
    }

    /// Newest-first list of snapshots.
    pub fn list(&self) -> Vec<SnapshotMeta> {
        let mut index = self.load_index();
        index.reverse();
        index
    }

    /// Wipe every snapshot (index + any tar archives).
    pub fn clear(&self) -> Result<()> {
        for meta in self.load_index() {
            if let Some(p) = meta.tar_path {
                let _ = fs::remove_file(p);
            }
        }
        self.save_index(&[])?;
        Ok(())
    }

    /// Keep the newest `retain` snapshots; delete older ones (and their tar
    /// archives). Git trees are left as unreferenced loose objects for git gc.
    fn prune(&self) -> Result<()> {
        let mut index = self.load_index();
        if index.len() <= self.retain {
            return Ok(());
        }
        let cutoff = index.len() - self.retain;
        let dropped: Vec<SnapshotMeta> = index.drain(0..cutoff).collect();
        for meta in dropped {
            if let Some(p) = meta.tar_path {
                let _ = fs::remove_file(p);
            }
        }
        self.save_index(&index)?;
        Ok(())
    }
}

/// 16-hex-char prefix of sha256(canonical path).
fn workspace_hash(canonical: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// ISO-8601 UTC timestamp (mirrors session::store formatting, second precision).
fn now_iso() -> String {
    use std::time::SystemTime;
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (t, d) = (secs % 86400, secs / 86400);
    let z = d + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        t / 3600,
        (t % 3600) / 60,
        t % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn cfg(retain: usize) -> SnapshotsConfig {
        SnapshotsConfig {
            enabled: true,
            auto_pre_patch: true,
            auto_pre_shell: true,
            auto_per_turn: false,
            retain,
            max_tar_mb: 512,
        }
    }

    #[test]
    fn workspace_hash_is_stable_and_short() {
        let p = Path::new("/tmp/some/workspace");
        let a = workspace_hash(p);
        let b = workspace_hash(p);
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, workspace_hash(Path::new("/tmp/other")));
    }

    #[test]
    fn index_persists_across_reopen() {
        let dir = TempDir::new().unwrap();
        let mgr = SnapshotManager::for_workspace(dir.path(), &cfg(20))
            .unwrap()
            .unwrap();
        let meta = SnapshotMeta {
            id: SnapshotId("snap_TEST".to_owned()),
            created_at: now_iso(),
            backend: Backend::Tar,
            reason: Reason::Manual,
            git_tree: None,
            git_head: None,
            tar_path: None,
            command: None,
        };
        mgr.save_index(&[meta]).unwrap();

        let reopened = SnapshotManager::for_workspace(dir.path(), &cfg(20))
            .unwrap()
            .unwrap();
        let listed = reopened.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id.as_str(), "snap_TEST");
    }

    #[test]
    fn prune_keeps_newest_retain() {
        let dir = TempDir::new().unwrap();
        let mgr = SnapshotManager::for_workspace(dir.path(), &cfg(3))
            .unwrap()
            .unwrap();
        let mut index = Vec::new();
        for i in 0..5 {
            index.push(SnapshotMeta {
                id: SnapshotId(format!("snap_{i}")),
                created_at: now_iso(),
                backend: Backend::Tar,
                reason: Reason::Manual,
                git_tree: None,
                git_head: None,
                tar_path: None,
                command: None,
            });
        }
        mgr.save_index(&index).unwrap();
        mgr.prune().unwrap();
        let kept: Vec<String> = mgr.load_index().iter().map(|m| m.id.0.clone()).collect();
        assert_eq!(kept, vec!["snap_2", "snap_3", "snap_4"]);
    }

    #[test]
    fn disabled_config_yields_none() {
        let dir = TempDir::new().unwrap();
        let mut c = cfg(20);
        c.enabled = false;
        assert!(SnapshotManager::for_workspace(dir.path(), &c).unwrap().is_none());
    }
}
