# M3 — Workspace Snapshots & Rollback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add workspace-level snapshots so a single `/snapshot restore <id>` rewinds the workspace to a known-good state, with auto-snapshots before risky agent operations.

**Architecture:** A `SnapshotManager` keyed by workspace-path hash stores snapshots under `~/.local/share/artui/snapshots/<hash>/` with a JSON index. A git backend (`git write-tree` from a throwaway index — captures untracked files, unlike `git stash create`) is used in git repos; a `tar.zst` backend is the fallback. Auto-snapshots fire in `run_turn` at the dispatch layer (which owns config + workspace context) before `apply_patch` and non-read-only `shell` calls. A `/snapshot` slash command exposes list/restore/clear.

**Tech Stack:** Rust, `git` (shelled out via `std::process::Command`), `tar = "0.4"`, `zstd = "0.13"`, `ignore` (gitignore-aware walk), `sha2`, `serde_json`, `ulid` — all but tar/zstd already deps.

**Design spec:** `docs/superpowers/specs/2026-06-01-m3-snapshots-rollback-design.md`

**Branch:** `phase-m3-snapshots` (already created; spec + docs cleanup already committed there).

---

## File Structure

| File | Responsibility |
|---|---|
| `src/snapshots/mod.rs` (new) | `SnapshotManager`, `SnapshotId`, `SnapshotMeta`, `Backend`, `Reason`, `SnapshotPolicy`; index load/save; retention/prune; backend dispatch; public API |
| `src/snapshots/git_backend.rs` (new) | git detection, `take` (temp-index write-tree), `restore` (read-tree + checkout + prune-additions) |
| `src/snapshots/tar_backend.rs` (new) | `ignore`-walked `tar.zst` capture + restore, size guard |
| `src/lib.rs` | `pub mod snapshots;` registration |
| `src/config/schema.rs` | `SnapshotsConfig` struct + `snapshots` field on `AppConfig` |
| `src/tools/shell.rs` | `pub fn is_read_only(command: &str) -> bool` + tests |
| `src/agent/loop.rs` | `AgentLoopConfig` snapshot fields + auto-snapshot before dispatch + per-turn |
| `src/app.rs` | `App.snapshots` field, `AppEvent::SnapshotSaved`, `ProviderRequest` fields, `/snapshot` slash command + table entry |

Each task is independently committable and ordered so the crate compiles + tests pass after every task.

---

## Task 1: Add tar + zstd dependencies

**Files:**
- Modify: `Cargo.toml:11-39` (`[dependencies]`)

- [ ] **Step 1: Add the two crates**

Add these two lines to `[dependencies]` in `Cargo.toml`, keeping the existing alphabetical ordering (`tar` after `tachyonfx`/`tokio` block — place it alphabetically; `zstd` at the end before nothing):

```toml
tar = "0.4"
zstd = "0.13"
```

Concretely, insert `tar = "0.4"` between `sha2 = "0.10"` (line 28) and `tachyonfx = "0.25"` (line 29), and `zstd = "0.13"` after `which = "7"` (line 39).

- [ ] **Step 2: Verify it resolves**

Run: `cargo fetch && cargo build 2>&1 | tail -5`
Expected: builds clean (the new crates are pulled; no code uses them yet, so no warnings about them).

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "build(m3): add tar + zstd deps for snapshot tar backend"
```

---

## Task 2: Snapshot core types + module skeleton

Creates the module with all shared types and a `SnapshotManager` whose backend methods are stubbed (`take`/`restore` `todo!()`-free but minimal), so later tasks fill in real backends. The index load/save + workspace hashing + retention are real and tested here.

**Files:**
- Create: `src/snapshots/mod.rs`
- Modify: `src/lib.rs` (add `pub mod snapshots;`)
- Test: inline `#[cfg(test)]` in `src/snapshots/mod.rs`

- [ ] **Step 1: Register the module**

In `src/lib.rs`, add `pub mod snapshots;` alongside the other top-level `pub mod` declarations (e.g. right after `pub mod session;` — find it with `rg -n "pub mod session;" src/lib.rs`).

- [ ] **Step 2: Write the failing test for workspace hashing + index round-trip**

Create `src/snapshots/mod.rs` with the full content below. It defines every shared type, the manager constructor (git/tar detection deferred to Task 3/4 via a `Backend` field set here), index persistence, retention, and the tests. Backend `take`/`restore` delegate to functions that will be implemented in Tasks 3–4; for now they return a "not yet implemented for this backend" error so the crate compiles and the *index/hash/retention* tests pass without touching a real backend.

```rust
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
        // Non-git temp dir → tar backend; we hand-write index entries to
        // exercise persistence without invoking a backend.
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
```

Note: this step references `SnapshotsConfig` (Task 5) and the backend modules (Tasks 3–4). To keep the crate compiling, **do Task 3, 4, and 5 stubs first if compiling now** — but the ordering below is arranged so you implement backends (3,4) and config (5) immediately after, and the crate is only built/tested at the end of Task 5. If you prefer strict per-task green, temporarily stub `SnapshotsConfig` and the backend fns; the canonical order here defers the first `cargo test` to Task 5 Step 4. **Do not commit Task 2 alone** — commit happens at the end of Task 5.

- [ ] **Step 3: (deferred) build** — see Task 5 Step 4.

---

## Task 3: Git backend

**Files:**
- Create: `src/snapshots/git_backend.rs`

- [ ] **Step 1: Write the backend**

Create `src/snapshots/git_backend.rs`:

```rust
//! Git snapshot backend. Captures the *entire* working tree — tracked AND
//! untracked files — by staging into a throwaway index and `git write-tree`.
//! Deliberately NOT `git stash create`, which omits untracked files (the
//! agent's apply_patch-created files are untracked until the user stages
//! them). Never touches the user's real index, stash list, or branches.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

/// True when `workspace` is inside a git work tree.
pub fn is_git_workspace(workspace: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false)
}

/// A temp index file path inside the repo's .git dir, unique per call.
fn temp_index(workspace: &Path) -> Result<std::path::PathBuf> {
    let git_dir = run(workspace, &["rev-parse", "--git-dir"], None)?;
    let git_dir = workspace.join(git_dir.trim());
    let name = format!("artui-snap-index-{}", ulid::Ulid::new());
    Ok(git_dir.join(name))
}

/// Capture: returns (tree_sha, head_sha?). Stages everything into a throwaway
/// index, writes a tree, then discards the index.
pub fn take(workspace: &Path) -> Result<(String, Option<String>)> {
    let index = temp_index(workspace)?;
    // Copy the current index as a starting point is unnecessary: `add -A`
    // against an empty temp index re-stages the full work tree.
    let res = (|| -> Result<(String, Option<String>)> {
        run(workspace, &["add", "-A"], Some(&index))
            .context("git add -A into temp index")?;
        let tree = run(workspace, &["write-tree"], Some(&index))
            .context("git write-tree")?
            .trim()
            .to_owned();
        let head = run(workspace, &["rev-parse", "HEAD"], None)
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        Ok((tree, head))
    })();
    let _ = std::fs::remove_file(&index);
    res
}

/// Restore: read the snapshot tree into a temp index, check it out over the
/// work tree, then delete files that exist now but are absent from the tree.
pub fn restore(workspace: &Path, tree: &str) -> Result<()> {
    let index = temp_index(workspace)?;
    let res = (|| -> Result<()> {
        run(workspace, &["read-tree", tree], Some(&index))
            .context("git read-tree snapshot")?;
        // Files present in the work tree but NOT in the snapshot tree must be
        // removed. Diff the snapshot tree against the work tree using the temp
        // index: added (A) paths are in the work tree but absent from the tree.
        let diff = run(
            workspace,
            &["diff", "--name-only", "--diff-filter=A", tree],
            Some(&index),
        )
        .context("git diff for additions")?;
        // Now overwrite tracked-by-snapshot files into the work tree.
        run(workspace, &["checkout-index", "-a", "-f"], Some(&index))
            .context("git checkout-index")?;
        for rel in diff.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let p = workspace.join(rel);
            let _ = std::fs::remove_file(&p);
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(&index);
    res
}

/// Run a git command in `workspace`, optionally with `GIT_INDEX_FILE` set.
/// Returns stdout on success; bails with stderr on failure.
fn run(workspace: &Path, args: &[&str], index: Option<&Path>) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(workspace).args(args);
    if let Some(idx) = index {
        cmd.env("GIT_INDEX_FILE", idx);
    }
    let out = cmd.output().context("spawn git")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?} failed");
    }

    fn init_repo(dir: &Path) {
        git(dir, &["init", "-q"]);
        git(dir, &["config", "user.email", "t@t"]);
        git(dir, &["config", "user.name", "t"]);
    }

    #[test]
    fn detects_git_workspace() {
        let dir = TempDir::new().unwrap();
        assert!(!is_git_workspace(dir.path()));
        init_repo(dir.path());
        assert!(is_git_workspace(dir.path()));
    }

    #[test]
    fn round_trip_rewinds_tracked_edit_and_removes_untracked() {
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        init_repo(p);
        fs::write(p.join("README.md"), "v1\n").unwrap();
        git(p, &["add", "-A"]);
        git(p, &["commit", "-qm", "init"]);

        // snapshot the known-good state
        let (tree, _head) = take(p).unwrap();

        // mutate: edit tracked file + create an untracked file
        fs::write(p.join("README.md"), "v2-broken\n").unwrap();
        fs::write(p.join("scratch.txt"), "junk\n").unwrap();

        restore(p, &tree).unwrap();

        assert_eq!(fs::read_to_string(p.join("README.md")).unwrap(), "v1\n");
        assert!(!p.join("scratch.txt").exists(), "untracked file should be removed on restore");
    }

    #[test]
    fn captures_untracked_file_in_snapshot() {
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        init_repo(p);
        // brand-new untracked file, no commit yet
        fs::write(p.join("new.txt"), "hello\n").unwrap();
        let (tree, head) = take(p).unwrap();
        assert!(head.is_none(), "no commits yet → no HEAD");

        // delete it, then restore should bring it back
        fs::remove_file(p.join("new.txt")).unwrap();
        restore(p, &tree).unwrap();
        assert_eq!(fs::read_to_string(p.join("new.txt")).unwrap(), "hello\n");
    }
}
```

- [ ] **Step 2: (deferred) build** — see Task 5 Step 4.

---

## Task 4: Tar backend

**Files:**
- Create: `src/snapshots/tar_backend.rs`

- [ ] **Step 1: Write the backend**

Create `src/snapshots/tar_backend.rs`:

```rust
//! Tar.zst snapshot backend for non-git workspaces. Walks the workspace with
//! the `ignore` crate (honors .gitignore/.ignore) plus a built-in exclude set,
//! archives into `<id>.tar.zst`. A size guard skips snapshots that would
//! exceed `max_tar_mb` of *uncompressed* input.

use std::fs::{self, File};
use std::path::Path;

use anyhow::{Context, Result};
use ignore::WalkBuilder;

const BUILTIN_EXCLUDES: &[&str] = &[".git", "target", "node_modules"];

fn is_excluded(rel: &Path) -> bool {
    rel.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        BUILTIN_EXCLUDES.contains(&s.as_ref())
    })
}

/// Capture the workspace into `archive`. Returns `Ok(None)` if the
/// uncompressed size would exceed `max_tar_mb` (snapshot skipped).
pub fn take(workspace: &Path, archive: &Path, max_tar_mb: u64) -> Result<Option<()>> {
    // First pass: sum sizes, bail early if over budget.
    let budget = max_tar_mb.saturating_mul(1024 * 1024);
    let mut total: u64 = 0;
    for entry in WalkBuilder::new(workspace).hidden(false).build().flatten() {
        let path = entry.path();
        let rel = match path.strip_prefix(workspace) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if rel.as_os_str().is_empty() || is_excluded(rel) {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            if meta.is_file() {
                total += meta.len();
                if total > budget {
                    return Ok(None);
                }
            }
        }
    }

    let file = File::create(archive).context("create snapshot archive")?;
    let encoder = zstd::Encoder::new(file, 0)
        .context("zstd encoder")?
        .auto_finish();
    let mut builder = tar::Builder::new(encoder);
    for entry in WalkBuilder::new(workspace).hidden(false).build().flatten() {
        let path = entry.path();
        let rel = match path.strip_prefix(workspace) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if rel.as_os_str().is_empty() || is_excluded(rel) {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_file() {
            let mut f = File::open(path).with_context(|| format!("open {}", path.display()))?;
            builder
                .append_file(rel, &mut f)
                .with_context(|| format!("archive {}", rel.display()))?;
        }
    }
    builder.finish().context("finish tar archive")?;
    Ok(Some(()))
}

/// Restore: delete the current (non-excluded) file set, then extract.
pub fn restore(workspace: &Path, archive: &Path) -> Result<()> {
    // Remove current non-excluded files (leave excluded dirs like .git alone).
    for entry in WalkBuilder::new(workspace).hidden(false).build().flatten() {
        let path = entry.path();
        let rel = match path.strip_prefix(workspace) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if rel.as_os_str().is_empty() || is_excluded(rel) {
            continue;
        }
        if entry.metadata().map(|m| m.is_file()).unwrap_or(false) {
            let _ = fs::remove_file(path);
        }
    }
    let file = File::open(archive).context("open snapshot archive")?;
    let decoder = zstd::Decoder::new(file).context("zstd decoder")?;
    let mut ar = tar::Archive::new(decoder);
    ar.unpack(workspace).context("extract snapshot archive")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn round_trip_rewinds_workspace() {
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        fs::write(p.join("a.txt"), "original\n").unwrap();
        fs::create_dir_all(p.join("sub")).unwrap();
        fs::write(p.join("sub/b.txt"), "nested\n").unwrap();

        let archive = TempDir::new().unwrap().path().join("snap.tar.zst");
        assert!(take(p, &archive, 512).unwrap().is_some());

        fs::write(p.join("a.txt"), "changed\n").unwrap();
        fs::write(p.join("c.txt"), "new junk\n").unwrap();

        restore(p, &archive).unwrap();

        assert_eq!(fs::read_to_string(p.join("a.txt")).unwrap(), "original\n");
        assert_eq!(fs::read_to_string(p.join("sub/b.txt")).unwrap(), "nested\n");
        assert!(!p.join("c.txt").exists(), "post-snapshot file should be gone after restore");
    }

    #[test]
    fn size_guard_skips_oversized_workspace() {
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        // ~2 MB file with a 1 MB budget → skipped.
        fs::write(p.join("big.bin"), vec![0u8; 2 * 1024 * 1024]).unwrap();
        let archive = TempDir::new().unwrap().path().join("snap.tar.zst");
        // max_tar_mb = 1
        assert!(take(p, &archive, 1).unwrap().is_none());
        assert!(!archive.exists(), "no archive written when over budget");
    }
}
```

- [ ] **Step 2: (deferred) build** — see Task 5 Step 4.

---

## Task 5: `[snapshots]` config block

**Files:**
- Modify: `src/config/schema.rs:5-16` (`AppConfig` struct + `Default`), and append the new struct.

- [ ] **Step 1: Add the field to `AppConfig`**

In `src/config/schema.rs`, add to the `AppConfig` struct (after `pub lsp: LspConfig,` on line 15):

```rust
    pub snapshots: SnapshotsConfig,
```

And in `impl Default for AppConfig` (after `lsp: LspConfig::default(),` ~line 35):

```rust
            snapshots: SnapshotsConfig::default(),
```

- [ ] **Step 2: Append the `SnapshotsConfig` struct**

Add at the end of `src/config/schema.rs`:

```rust
/// `[snapshots]` — workspace snapshot & rollback safety net (Phase M3).
///
/// When `enabled = true` (default), artui keeps workspace snapshots under
/// `~/.local/share/artui/snapshots/<workspace_hash>/` and can auto-snapshot
/// before risky agent operations. `/snapshot list|restore <id>|clear` manage
/// them. Set `enabled = false` to disable the subsystem entirely.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SnapshotsConfig {
    /// Master switch. When false, no manager is constructed and no
    /// auto-snapshots fire.
    pub enabled: bool,
    /// Snapshot before each `apply_patch`.
    pub auto_pre_patch: bool,
    /// Snapshot before each non-read-only `shell` command.
    pub auto_pre_shell: bool,
    /// Snapshot once at the start of every agent turn.
    pub auto_per_turn: bool,
    /// Keep the newest N snapshots; older ones are auto-pruned.
    pub retain: usize,
    /// Skip a tar-backend snapshot when the workspace exceeds this many MB
    /// (uncompressed). Guards against archiving giant build dirs.
    pub max_tar_mb: u64,
}

impl Default for SnapshotsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_pre_patch: true,
            auto_pre_shell: true,
            auto_per_turn: false,
            retain: 20,
            max_tar_mb: 512,
        }
    }
}
```

- [ ] **Step 3: Make backend submodules visible to `mod.rs`**

The `mod git_backend;` / `mod tar_backend;` lines were declared in Task 2's `mod.rs`. No change needed here — just confirming the three snapshot files now all exist.

- [ ] **Step 4: Build + run the whole snapshots + config test suite**

Run: `cargo test snapshots:: config::schema 2>&1 | tail -25`
(If your shell splits that oddly, run `cargo test --lib 2>&1 | tail -30`.)
Expected: all snapshot tests pass — `workspace_hash_is_stable_and_short`, `index_persists_across_reopen`, `prune_keeps_newest_retain`, `disabled_config_yields_none`, `detects_git_workspace`, `round_trip_rewinds_tracked_edit_and_removes_untracked`, `captures_untracked_file_in_snapshot`, `round_trip_rewinds_workspace`, `size_guard_skips_oversized_workspace`.

- [ ] **Step 5: Commit Tasks 2–5 together**

```bash
git add src/snapshots/ src/lib.rs src/config/schema.rs
git commit -m "feat(m3): snapshot core, git + tar backends, [snapshots] config

SnapshotManager keyed by workspace-path hash under
~/.local/share/artui/snapshots/<hash>/. Git backend captures the full
working tree (incl. untracked) via a throwaway-index write-tree, not
git stash create. Tar.zst fallback with .gitignore-aware walk + size
guard. JSON index with retention. Backends round-trip tested."
```

---

## Task 6: `shell::is_read_only` classifier

**Files:**
- Modify: `src/tools/shell.rs` (add fn near `classify_deny` at line 234; add tests in the existing `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing tests**

In `src/tools/shell.rs`, inside the existing `#[cfg(test)] mod tests` block (after `classifier_denies_rm_rf_root`), add:

```rust
    #[test]
    fn read_only_commands_recognized() {
        for c in [
            "ls", "ls -la", "cat foo.rs", "grep -r x .", "rg pattern",
            "find . -name '*.rs'", "pwd", "git status", "git log --oneline",
            "git diff HEAD~1", "git show", "wc -l file", "head -n5 f",
        ] {
            assert!(is_read_only(c), "expected read-only: {c}");
        }
    }

    #[test]
    fn mutating_commands_not_read_only() {
        for c in [
            "rm foo", "mv a b", "cargo build", "git commit -m x",
            "git checkout main", "touch new", "echo hi > f",
            "ls && rm x", "cat f | tee g", "make install", "npm i",
        ] {
            assert!(!is_read_only(c), "expected mutating: {c}");
        }
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib tools::shell::tests::read_only 2>&1 | tail -15`
Expected: FAIL — `cannot find function is_read_only in this scope`.

- [ ] **Step 3: Implement `is_read_only`**

Add this function in `src/tools/shell.rs` immediately after `classify_deny` (after line 260):

```rust
/// Conservative read-only classifier: returns true only when the command is a
/// single, recognizably non-mutating invocation. Anything with shell operators
/// (`|`, `&&`, `;`, `>`, `<`, backticks, `$(`) or an unknown leading token is
/// treated as mutating so the caller snapshots first. Used by the agent loop to
/// decide whether a `shell` call needs a pre-snapshot.
pub fn is_read_only(command: &str) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return true; // nothing to snapshot
    }
    // Any shell metacharacter that could chain/redirect/expand → mutating.
    if trimmed
        .chars()
        .any(|c| matches!(c, '|' | '&' | ';' | '>' | '<' | '`'))
        || trimmed.contains("$(")
    {
        return false;
    }
    let mut tokens = trimmed.split_whitespace();
    let Some(cmd) = tokens.next() else {
        return true;
    };
    // git is read-only only for a known read-only subcommand.
    if cmd == "git" {
        return matches!(
            tokens.next(),
            Some(
                "status" | "log" | "diff" | "show" | "blame" | "branch" | "remote"
                    | "rev-parse" | "ls-files" | "describe" | "config"
            )
        );
    }
    const READ_ONLY: &[&str] = &[
        "ls", "pwd", "cat", "head", "tail", "less", "more", "file", "stat", "wc",
        "grep", "rg", "egrep", "fgrep", "find", "fd", "echo", "which", "type",
        "printenv", "env", "date", "whoami", "id", "du", "df", "tree",
    ];
    READ_ONLY.contains(&cmd)
}
```

Note: `env` and `echo` are listed as read-only for the bare/simple form; the metacharacter guard above already rejects `echo hi > f` (redirect) and `env FOO=bar mutate` stays read-only only if the rest is read-only — acceptable since the guard catches the dangerous forms and a false "read-only" at worst skips a snapshot for a harmless command.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib tools::shell::tests 2>&1 | tail -15`
Expected: PASS — both new tests plus existing `classifier_*` tests green.

- [ ] **Step 5: Commit**

```bash
git add src/tools/shell.rs
git commit -m "feat(m3): add shell::is_read_only classifier for snapshot gating"
```

---

## Task 7: Auto-snapshot in the agent loop

**Files:**
- Modify: `src/agent/loop.rs` — `AgentLoopConfig` struct (lines 22-59), its `Default` (61-79), and the dispatch loop (around 303-410).

- [ ] **Step 1: Add fields to `AgentLoopConfig`**

In `src/agent/loop.rs`, add to the `AgentLoopConfig` struct (after `pub max_nudges_per_turn: u32,` line 58):

```rust
    /// Snapshot manager for auto-snapshots before risky tools. `None`
    /// disables snapshots (matches `[snapshots] enabled = false`).
    pub snapshots: Option<std::sync::Arc<crate::snapshots::SnapshotManager>>,
    /// Which auto-snapshots are enabled.
    pub snapshot_policy: crate::snapshots::SnapshotPolicy,
```

In `impl Default for AgentLoopConfig` (after `max_nudges_per_turn: 2,` line 76):

```rust
            snapshots: None,
            snapshot_policy: crate::snapshots::SnapshotPolicy {
                auto_pre_patch: false,
                auto_pre_shell: false,
                auto_per_turn: false,
            },
```

(Default all-false so existing tests are unaffected; the App wires real values in Task 8.)

- [ ] **Step 2: Add a helper near the top of the loop module**

Add this free function in `src/agent/loop.rs` (after the imports, before `pub struct AgentLoopConfig`):

```rust
/// Take an auto-snapshot if a manager is present, emitting `SnapshotSaved` on
/// success. Never fails the turn — a snapshot is a safety net, not a gate.
async fn maybe_snapshot(
    config: &AgentLoopConfig,
    reason: crate::snapshots::Reason,
    command: Option<String>,
    event_tx: &mpsc::Sender<AppEvent>,
) {
    let Some(mgr) = config.snapshots.as_ref() else {
        return;
    };
    match mgr.take(reason, command) {
        Ok(Some(id)) => {
            let _ = event_tx
                .send(AppEvent::SnapshotSaved { id: id.0 })
                .await;
        }
        Ok(None) => {} // skipped (size guard) — silent
        Err(e) => tracing::warn!("snapshot failed: {e:#}"),
    }
}
```

- [ ] **Step 3: Per-turn snapshot at loop entry**

In `run_turn`, immediately after `let mut nudges_used: u32 = 0;` (line 101) and before `loop {`, add:

```rust
    if config.snapshot_policy.auto_per_turn {
        maybe_snapshot(config, crate::snapshots::Reason::PerTurn, None, &event_tx).await;
    }
```

- [ ] **Step 4: Pre-dispatch snapshot inside the tool loop**

In the `for call in &tool_calls {` loop, immediately after the `fire_hooks(... PreToolUse ...)` call (after line 318, before the `// ── Permission gate` comment), add:

```rust
            // ── Auto-snapshot before risky tools ───────────────────
            // Fires before the permission gate so it covers every
            // allow path. apply_patch always; shell only when the
            // command is not read-only.
            if call.name == "apply_patch" && config.snapshot_policy.auto_pre_patch {
                maybe_snapshot(config, crate::snapshots::Reason::PrePatch, None, &event_tx).await;
            } else if call.name == "shell" && config.snapshot_policy.auto_pre_shell {
                let cmd = call
                    .arguments
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !crate::tools::shell::is_read_only(cmd) {
                    maybe_snapshot(
                        config,
                        crate::snapshots::Reason::PreShell,
                        Some(cmd.to_owned()),
                        &event_tx,
                    )
                    .await;
                }
            }
```

- [ ] **Step 5: Build (compile check — `AppEvent::SnapshotSaved` lands in Task 8)**

`AppEvent::SnapshotSaved` does not exist yet, so this task does NOT compile alone. It compiles after Task 8 Step 1. **Do not build or commit Task 7 alone — it is committed together with Task 8.** Proceed to Task 8.

---

## Task 8: `AppEvent::SnapshotSaved`, App wiring, `/snapshot` command

**Files:**
- Modify: `src/app.rs` — `AppEvent` enum (193-212), `ProviderRequest` struct (366-385) + build site (825-845), `App` struct (455-475) + constructor (540-560), `SLASH_COMMANDS` table (237), `run_slash_command` (1674), and the `AppEvent` handler.
- Modify: `src/lib.rs:702-712` — pass new fields into `AgentLoopConfig`.

- [ ] **Step 1: Add the `SnapshotSaved` event variant**

In `src/app.rs`, add to the `AppEvent` enum (after the `TodoUpdate(...)` variant, ~line 211):

```rust
    /// Phase M3 — an auto-snapshot was taken before a risky operation.
    /// The TUI shows a dim, copy-able `Snapshot saved: snap_…` line.
    SnapshotSaved { id: String },
```

- [ ] **Step 2: Add `snapshots` to `App` struct + constructor**

In the `App` struct (after `pub lsp_manager: Option<...>,` ~line 470):

```rust
    /// Phase M3 — workspace snapshot manager. `None` when `[snapshots]
    /// enabled = false` or the data dir can't be resolved.
    pub snapshots: Option<std::sync::Arc<crate::snapshots::SnapshotManager>>,
```

In the `App` constructor (after `lsp_manager: None,` ~line 554):

```rust
            snapshots: crate::snapshots::SnapshotManager::for_workspace(
                &std::env::current_dir().unwrap_or_default(),
                &config.snapshots,
            )
            .ok()
            .flatten()
            .map(std::sync::Arc::new),
```

Note: `config` is moved into the struct literal on the line after `status:`; `config.snapshots` is read here *before* that move because struct fields evaluate top-to-bottom and `snapshots:` appears before `config,`? — NOT guaranteed. To be safe, read it from the local `config` binding which is still in scope at this point in the constructor (the `config,` shorthand consumes it). Place this `snapshots:` initializer **above** the `config,` line in the literal. If the borrow checker complains, clone: `&config.snapshots.clone()`.

- [ ] **Step 3: Add fields to `ProviderRequest` + build site**

In the `ProviderRequest` struct (after `pub lsp_diagnostics_timeout_ms: ...,` — find the last field before the closing brace, ~line 386):

```rust
    /// Phase M3 — snapshot manager handed to the agent loop for
    /// auto-snapshots. `None` disables snapshots.
    pub snapshots: Option<std::sync::Arc<crate::snapshots::SnapshotManager>>,
    /// Phase M3 — which auto-snapshots are enabled.
    pub snapshot_policy: crate::snapshots::SnapshotPolicy,
```

At the `ProviderRequest { ... }` build site (after `lsp_diagnostics_timeout_ms: self.config.lsp.diagnostics_timeout_ms,` ~line 845):

```rust
            snapshots: self.snapshots.clone(),
            snapshot_policy: crate::snapshots::SnapshotPolicy::from_config(&self.config.snapshots),
```

- [ ] **Step 4: Thread into `AgentLoopConfig` in `lib.rs`**

In `src/lib.rs`, in the `AgentLoopConfig { ... }` literal (after `lsp_diagnostics_timeout_ms: request.lsp_diagnostics_timeout_ms,` line 711):

```rust
                    snapshots: request.snapshots,
                    snapshot_policy: request.snapshot_policy,
```

- [ ] **Step 5: Register `/snapshot` in the command table**

In `src/app.rs`, change `pub const SLASH_COMMANDS: [SlashCommand; 15]` to `[SlashCommand; 16]` (line 237) and add an entry (place it after the `/permissions` entry):

```rust
    SlashCommand {
        name: "/snapshot",
        description: "List, restore, or clear workspace snapshots",
    },
```

- [ ] **Step 6: Handle `/snapshot` in `run_slash_command`**

In `run_slash_command` (`src/app.rs:1674`), add match arms. Because the subcommands take an argument, match with a guard before the catch-all. Add this block among the other arms (e.g. after the `/permissions` arm):

```rust
            "/snapshot" | "/snapshot list" => {
                let mut lines = vec!["# Snapshots".to_owned()];
                match &self.snapshots {
                    None => lines.push(
                        "(snapshots disabled — set `[snapshots] enabled = true`)".to_owned(),
                    ),
                    Some(mgr) => {
                        let list = mgr.list();
                        if list.is_empty() {
                            lines.push("(no snapshots yet)".to_owned());
                        } else {
                            for m in list {
                                lines.push(format!(
                                    "- `{}`  {}  {}  {}",
                                    m.id,
                                    m.created_at,
                                    m.reason.label(),
                                    match m.backend {
                                        crate::snapshots::Backend::Git => "git",
                                        crate::snapshots::Backend::Tar => "tar",
                                    }
                                ));
                            }
                            lines.push(
                                "\nRestore with `/snapshot restore <id>`.".to_owned(),
                            );
                        }
                    }
                }
                self.transcript
                    .push(Message::new(Role::Assistant, lines.join("\n")));
                self.mode = UiMode::Input;
                SlashCommandResult::Handled(None)
            }
            "/snapshot clear" => {
                let msg = match &self.snapshots {
                    None => "snapshots are disabled".to_owned(),
                    Some(mgr) => {
                        let n = mgr.list().len();
                        match mgr.clear() {
                            Ok(()) => format!("Cleared {n} snapshot(s)."),
                            Err(e) => format!("clear failed: {e}"),
                        }
                    }
                };
                self.transcript.push(Message::new(Role::Assistant, msg));
                self.mode = UiMode::Input;
                SlashCommandResult::Handled(None)
            }
            other if other.starts_with("/snapshot restore ") => {
                let id = other.trim_start_matches("/snapshot restore ").trim();
                let msg = match &self.snapshots {
                    None => "snapshots are disabled".to_owned(),
                    Some(mgr) => {
                        let sid = crate::snapshots::SnapshotId(id.to_owned());
                        match mgr.restore(&sid) {
                            Ok(()) => format!("Rewound workspace to `{id}`."),
                            Err(e) => format!("restore failed: {e}"),
                        }
                    }
                };
                self.transcript.push(Message::new(Role::Assistant, msg));
                self.mode = UiMode::Input;
                SlashCommandResult::Handled(None)
            }
```

- [ ] **Step 7: Render the `SnapshotSaved` event**

Find where `AppEvent` variants are handled (search: `rg -n "AppEvent::TodoUpdate|AppEvent::LspDiagnostics" src/`). In the same handler (likely `src/lib.rs` or `App::handle_event` in `src/app.rs`), add an arm:

```rust
            AppEvent::SnapshotSaved { id } => {
                app.status = format!("Snapshot saved: {id}");
            }
```

Match the exact local variable name used by surrounding arms (`app` vs `self`) — check the neighbours. If the handler is a method on `App`, use `self.status = ...`.

- [ ] **Step 8: Build the whole crate**

Run: `cargo build 2>&1 | tail -20`
Expected: clean build. Fix any borrow/move issue per the notes in Step 2 (read `config.snapshots` before `config` is moved).

- [ ] **Step 9: Run the full test suite**

Run: `cargo test 2>&1 | tail -25`
Expected: all green, including the snapshot + shell tests and the existing suite (no regressions in app/loop tests).

- [ ] **Step 10: Commit Tasks 7 + 8 together**

```bash
git add src/agent/loop.rs src/app.rs src/lib.rs
git commit -m "feat(m3): auto-snapshot in agent loop + /snapshot command + UI hint

run_turn takes a pre-dispatch snapshot before apply_patch and
non-read-only shell calls (and per-turn when configured), emitting
AppEvent::SnapshotSaved. App constructs the SnapshotManager from
[snapshots] config and threads it through ProviderRequest. /snapshot
list|restore <id>|clear manage snapshots from the TUI."
```

---

## Task 9: Integration test — end-to-end git round-trip via the manager

**Files:**
- Create: `tests/snapshots_integration.rs`

- [ ] **Step 1: Write the integration test**

Create `tests/snapshots_integration.rs`:

```rust
//! End-to-end: SnapshotManager over a real git workspace — the acceptance
//! criterion from issue #19 (apply_patch-style edit → snapshot → mutate →
//! restore rewinds).

use std::fs;
use std::path::Path;
use std::process::Command;

use artui::config::schema::SnapshotsConfig;
use artui::snapshots::{Reason, SnapshotManager};

fn git(dir: &Path, args: &[&str]) {
    assert!(
        Command::new("git").arg("-C").arg(dir).args(args).output().unwrap().status.success(),
        "git {args:?}"
    );
}

fn cfg() -> SnapshotsConfig {
    SnapshotsConfig { enabled: true, auto_pre_patch: true, auto_pre_shell: true, auto_per_turn: false, retain: 20, max_tar_mb: 512 }
}

#[test]
fn git_manager_round_trip() {
    let dir = tempfile::TempDir::new().unwrap();
    let p = dir.path();
    git(p, &["init", "-q"]);
    git(p, &["config", "user.email", "t@t"]);
    git(p, &["config", "user.name", "t"]);
    fs::write(p.join("README.md"), "good\n").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "init"]);

    let mgr = SnapshotManager::for_workspace(p, &cfg()).unwrap().unwrap();
    let id = mgr.take(Reason::Manual, None).unwrap().unwrap();

    fs::write(p.join("README.md"), "bad\n").unwrap();
    fs::write(p.join("extra.txt"), "x\n").unwrap();

    mgr.restore(&id).unwrap();
    assert_eq!(fs::read_to_string(p.join("README.md")).unwrap(), "good\n");
    assert!(!p.join("extra.txt").exists());
}

#[test]
fn tar_manager_round_trip_non_git() {
    let dir = tempfile::TempDir::new().unwrap();
    let p = dir.path();
    fs::write(p.join("a.txt"), "good\n").unwrap();

    let mgr = SnapshotManager::for_workspace(p, &cfg()).unwrap().unwrap();
    assert_eq!(mgr.backend(), artui::snapshots::Backend::Tar);
    let id = mgr.take(Reason::Manual, None).unwrap().unwrap();

    fs::write(p.join("a.txt"), "bad\n").unwrap();
    mgr.restore(&id).unwrap();
    assert_eq!(fs::read_to_string(p.join("a.txt")).unwrap(), "good\n");
}
```

- [ ] **Step 2: Confirm public visibility**

The integration test needs `artui::config::schema::SnapshotsConfig` and `artui::snapshots::{SnapshotManager, Reason, Backend}` public. `pub mod snapshots;` (Task 2) and the `pub` types already satisfy this. Confirm `config` and `schema` are `pub` (search: `rg -n "pub mod config|pub mod schema|pub mod schema;" src/config/mod.rs src/lib.rs`). If `schema` is private, add `pub mod schema;` in `src/config/mod.rs`.

- [ ] **Step 3: Run the integration test**

Run: `cargo test --test snapshots_integration 2>&1 | tail -15`
Expected: PASS — both `git_manager_round_trip` and `tar_manager_round_trip_non_git`.

- [ ] **Step 4: Commit**

```bash
git add tests/snapshots_integration.rs src/config/mod.rs
git commit -m "test(m3): end-to-end snapshot round-trip (git + tar) integration"
```

---

## Task 10: Docs — README + config reference + CHANGELOG

**Files:**
- Modify: `README.md` (the feature list / config section — find with `rg -n "snapshot|Releases|## Config" README.md`)
- Modify: `docs/changelogs/CHANGELOG.md` (top unreleased entry)

- [ ] **Step 1: Add a Snapshots note to README**

Add a short subsection to `README.md` near the existing feature documentation (match the surrounding heading style):

```markdown
### Workspace snapshots

artui auto-snapshots the workspace before risky operations (`apply_patch`,
non-read-only `shell`) so you can rewind in one command. Snapshots use
`git write-tree` in a git repo (captures untracked files too) or a `tar.zst`
archive otherwise, stored under `~/.local/share/artui/snapshots/`.

- `/snapshot` or `/snapshot list` — list snapshots
- `/snapshot restore <id>` — rewind the workspace to a snapshot
- `/snapshot clear` — delete all snapshots

Configure under `[snapshots]` in `~/.config/artui/config.toml`
(`enabled`, `auto_pre_patch`, `auto_pre_shell`, `auto_per_turn`, `retain`,
`max_tar_mb`). Restore only ever rewinds artui's own snapshots — never your
manual `git stash` entries or branches.
```

- [ ] **Step 2: Add a CHANGELOG entry**

At the top of `docs/changelogs/CHANGELOG.md`, under the unreleased/next-version heading (match the existing format), add:

```markdown
- **Workspace snapshots & rollback (Phase M3).** Auto-snapshot before
  `apply_patch` and non-read-only `shell`; `git write-tree` backend (captures
  untracked files) with a `tar.zst` fallback for non-git workspaces. New
  `/snapshot list|restore <id>|clear` commands and `[snapshots]` config block.
```

- [ ] **Step 3: Commit**

```bash
git add README.md docs/changelogs/CHANGELOG.md
git commit -m "docs(m3): document workspace snapshots & /snapshot command"
```

---

## Task 11: Final verification + clippy

- [ ] **Step 1: Full build, test, clippy, fmt**

Run:
```bash
cargo fmt --all
cargo clippy --all-targets 2>&1 | tail -30
cargo test 2>&1 | tail -25
```
Expected: `fmt` clean, `clippy` no new warnings in `src/snapshots/`, all tests pass.

- [ ] **Step 2: Manual smoke (optional but recommended)**

In a throwaway git repo, run artui, have the agent edit a file (triggers a pre-patch snapshot → `Snapshot saved: snap_…` status), then `/snapshot list` and `/snapshot restore <id>`; confirm the file rewinds.

- [ ] **Step 3: Commit any fmt/clippy fixups**

```bash
git add -A
git commit -m "chore(m3): fmt + clippy cleanup for snapshots" || echo "nothing to commit"
```

---

## Self-Review notes (resolved during planning)

- **Spec coverage:** take/restore (T3/T4), workspace-keyed storage (T2), git+tar backends (T3/T4), auto pre-patch/pre-shell/per-turn (T7), `/snapshot` list/restore/clear (T8), retention (T2 `prune`, tested), TUI hint (T8 `SnapshotSaved`), `[snapshots]` config (T5), read-only allowlist (T6), tests incl. git+tar round-trip + retention + read-only + hash/persistence + size guard (T2/T3/T4/T6/T9). Deferred items (subagent cascade, sync, diff browser, encryption) intentionally absent.
- **Type consistency:** `SnapshotId(pub String)` accessed as `.0` / `.as_str()` consistently; `Reason::label()`, `Backend` match arms, `SnapshotPolicy::from_config`, `SnapshotManager::{for_workspace,take,restore,list,clear}` signatures match across T2/T7/T8/T9. `take` returns `Result<Option<SnapshotId>>` everywhere (size-guard skip = `Ok(None)`).
- **Compile ordering caveat (called out in-task):** Tasks 2–5 are committed together (first `cargo test` at T5S4); Tasks 7–8 are committed together (`AppEvent::SnapshotSaved` defined in T8 but referenced in T7). Every commit leaves the crate green.
- **Known fragile spot flagged for the implementer:** the `App` constructor must read `config.snapshots` before the `config,` field-shorthand moves `config` (T8S2 note).
