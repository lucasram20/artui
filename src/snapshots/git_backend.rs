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
        // Stage the *current* working tree into the temp index, then diff it
        // against the snapshot tree to find files added since the snapshot —
        // these must be removed after the checkout below.
        run(workspace, &["add", "-A"], Some(&index))
            .context("git add -A into temp index")?;
        let diff = run(
            workspace,
            &["diff", "--cached", "--name-only", "--diff-filter=A", tree],
            Some(&index),
        )
        .context("git diff for additions")?;
        run(workspace, &["read-tree", tree], Some(&index))
            .context("git read-tree snapshot")?;
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

        let (tree, _head) = take(p).unwrap();

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
        fs::write(p.join("new.txt"), "hello\n").unwrap();
        let (tree, head) = take(p).unwrap();
        assert!(head.is_none(), "no commits yet → no HEAD");

        fs::remove_file(p.join("new.txt")).unwrap();
        restore(p, &tree).unwrap();
        assert_eq!(fs::read_to_string(p.join("new.txt")).unwrap(), "hello\n");
    }
}
