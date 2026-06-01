use std::fs;
use std::path::Path;
use std::process::Command;

use artui::config::SnapshotsConfig;
use artui::snapshots::{Backend, Reason, SnapshotManager};
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

fn git(workspace: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("failed to spawn git {args:?}: {err}"));
    assert!(
        output.status.success(),
        "git {args:?} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_repo(workspace: &Path) {
    git(workspace, &["init", "-q"]);
    git(
        workspace,
        &["config", "user.email", "snapshots@example.test"],
    );
    git(workspace, &["config", "user.name", "Snapshot Tests"]);
}

#[test]
fn git_backend_round_trips_tracked_edits_and_post_snapshot_untracked_files() {
    let dir = TempDir::new().unwrap();
    let workspace = dir.path();
    init_repo(workspace);

    fs::write(workspace.join("tracked.txt"), "original\n").unwrap();
    git(workspace, &["add", "tracked.txt"]);
    git(workspace, &["commit", "-qm", "initial"]);

    let mgr = SnapshotManager::for_workspace(workspace, &cfg(20))
        .unwrap()
        .unwrap();
    assert_eq!(mgr.backend(), Backend::Git);
    mgr.clear().unwrap();

    let snap = mgr
        .take(Reason::Manual, Some("integration git".to_owned()))
        .unwrap()
        .expect("git snapshot should be created");

    fs::write(workspace.join("tracked.txt"), "mutated\n").unwrap();
    fs::write(workspace.join("post_snapshot.txt"), "remove me\n").unwrap();

    mgr.restore(&snap).unwrap();

    assert_eq!(
        fs::read_to_string(workspace.join("tracked.txt")).unwrap(),
        "original\n"
    );
    assert!(
        !workspace.join("post_snapshot.txt").exists(),
        "restore should remove files added after the snapshot"
    );
    mgr.clear().unwrap();
}

#[test]
fn tar_backend_round_trips_non_git_workspace() {
    let dir = TempDir::new().unwrap();
    let workspace = dir.path();
    fs::create_dir_all(workspace.join("nested")).unwrap();
    fs::write(workspace.join("file.txt"), "original\n").unwrap();
    fs::write(workspace.join("nested/child.txt"), "nested original\n").unwrap();

    let mgr = SnapshotManager::for_workspace(workspace, &cfg(20))
        .unwrap()
        .unwrap();
    assert_eq!(mgr.backend(), Backend::Tar);
    mgr.clear().unwrap();

    let snap = mgr
        .take(Reason::Manual, Some("integration tar".to_owned()))
        .unwrap()
        .expect("tar snapshot should be created");

    fs::write(workspace.join("file.txt"), "mutated\n").unwrap();
    fs::write(workspace.join("nested/child.txt"), "nested mutated\n").unwrap();
    fs::write(workspace.join("new.txt"), "remove me\n").unwrap();

    mgr.restore(&snap).unwrap();

    assert_eq!(
        fs::read_to_string(workspace.join("file.txt")).unwrap(),
        "original\n"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("nested/child.txt")).unwrap(),
        "nested original\n"
    );
    assert!(
        !workspace.join("new.txt").exists(),
        "restore should remove files added after the snapshot"
    );
    mgr.clear().unwrap();
}

#[test]
fn public_api_prunes_to_retention_limit() {
    let dir = TempDir::new().unwrap();
    let workspace = dir.path();
    fs::write(workspace.join("state.txt"), "v1\n").unwrap();

    let mgr = SnapshotManager::for_workspace(workspace, &cfg(2))
        .unwrap()
        .unwrap();
    assert_eq!(mgr.backend(), Backend::Tar);
    mgr.clear().unwrap();

    let first = mgr
        .take(Reason::Manual, Some("first".to_owned()))
        .unwrap()
        .expect("first snapshot should be created");
    let first_archive = mgr
        .list()
        .into_iter()
        .find(|meta| meta.id == first)
        .and_then(|meta| meta.tar_path)
        .expect("first tar snapshot should record archive path");
    assert!(first_archive.exists());

    fs::write(workspace.join("state.txt"), "v2\n").unwrap();
    let second = mgr
        .take(Reason::Manual, Some("second".to_owned()))
        .unwrap()
        .expect("second snapshot should be created");

    fs::write(workspace.join("state.txt"), "v3\n").unwrap();
    let third = mgr
        .take(Reason::Manual, Some("third".to_owned()))
        .unwrap()
        .expect("third snapshot should be created");

    let listed_ids: Vec<_> = mgr.list().into_iter().map(|meta| meta.id).collect();
    assert_eq!(listed_ids, vec![third, second]);
    assert!(
        !first_archive.exists(),
        "pruning should remove the archive for dropped tar snapshots"
    );
    assert!(
        mgr.restore(&first).is_err(),
        "pruned snapshots should no longer be restorable through the public API"
    );
    mgr.clear().unwrap();
}
