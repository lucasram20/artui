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

/// Yield `(absolute_path, relative_path)` for every non-excluded entry under
/// `workspace`, honoring .gitignore (via the `ignore` crate) and the builtin
/// exclude set.
fn walk_included(
    workspace: &Path,
) -> impl Iterator<Item = (std::path::PathBuf, std::path::PathBuf)> + '_ {
    WalkBuilder::new(workspace)
        .hidden(false)
        .build()
        .flatten()
        .filter_map(move |entry| {
            let path = entry.path().to_path_buf();
            let rel = path.strip_prefix(workspace).ok()?.to_path_buf();
            if rel.as_os_str().is_empty() || is_excluded(&rel) {
                return None;
            }
            Some((path, rel))
        })
}

/// Capture the workspace into `archive`. Returns `Ok(None)` if the
/// uncompressed size would exceed `max_tar_mb` (snapshot skipped).
pub fn take(workspace: &Path, archive: &Path, max_tar_mb: u64) -> Result<Option<()>> {
    let budget = max_tar_mb.saturating_mul(1024 * 1024);
    let mut total: u64 = 0;
    for (path, _rel) in walk_included(workspace) {
        if let Ok(meta) = path.symlink_metadata() {
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
    for (path, rel) in walk_included(workspace) {
        let meta = match path.symlink_metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_file() {
            let mut f = File::open(&path).with_context(|| format!("open {}", path.display()))?;
            builder
                .append_file(&rel, &mut f)
                .with_context(|| format!("archive {}", rel.display()))?;
        }
    }
    builder.finish().context("finish tar archive")?;
    Ok(Some(()))
}

/// Restore: delete the current (non-excluded) file set, then extract.
pub fn restore(workspace: &Path, archive: &Path) -> Result<()> {
    for (path, _rel) in walk_included(workspace) {
        if path
            .symlink_metadata()
            .map(|m| m.is_file())
            .unwrap_or(false)
        {
            let _ = fs::remove_file(&path);
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

        let adir = TempDir::new().unwrap();
        let archive = adir.path().join("snap.tar.zst");
        assert!(take(p, &archive, 512).unwrap().is_some());

        fs::write(p.join("a.txt"), "changed\n").unwrap();
        fs::write(p.join("c.txt"), "new junk\n").unwrap();

        restore(p, &archive).unwrap();

        assert_eq!(fs::read_to_string(p.join("a.txt")).unwrap(), "original\n");
        assert_eq!(fs::read_to_string(p.join("sub/b.txt")).unwrap(), "nested\n");
        assert!(
            !p.join("c.txt").exists(),
            "post-snapshot file should be gone after restore"
        );
    }

    #[test]
    fn size_guard_skips_oversized_workspace() {
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        fs::write(p.join("big.bin"), vec![0u8; 2 * 1024 * 1024]).unwrap();
        let adir = TempDir::new().unwrap();
        let archive = adir.path().join("snap.tar.zst");
        assert!(take(p, &archive, 1).unwrap().is_none());
        assert!(!archive.exists(), "no archive written when over budget");
    }
}
