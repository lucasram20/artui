//! Workspace indexer — symbol table + line chunks (Phase M6).

mod symbols;
mod text;

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

pub use symbols::SymbolHit;
pub use text::TextHit;

use crate::config::IndexConfig;

pub struct WorkspaceIndex {
    conn: Mutex<rusqlite::Connection>,
}

impl WorkspaceIndex {
    pub fn open(workspace: &Path, cfg: &IndexConfig) -> Result<Option<Arc<Self>>> {
        if !cfg.enabled {
            return Ok(None);
        }
        let data_dir = directories::ProjectDirs::from("", "", "artui")
            .context("resolve data dir")?
            .data_dir()
            .to_path_buf();
        let base = data_dir.join("index").join(workspace_hash(workspace));
        std::fs::create_dir_all(&base).context("create index dir")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700));
        }
        let db_path = base.join("index.db");
        let conn = rusqlite::Connection::open(&db_path).context("open index db")?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS symbols (
               path TEXT NOT NULL,
               name TEXT NOT NULL,
               line INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
             CREATE TABLE IF NOT EXISTS chunks (
               path TEXT NOT NULL,
               line INTEGER NOT NULL,
               body TEXT NOT NULL
             );",
        )
        .context("index schema")?;

        let index = Arc::new(Self {
            conn: Mutex::new(conn),
        });
        index.rebuild(workspace, cfg.max_size_mb)?;
        Ok(Some(index))
    }

    pub fn rebuild(&self, workspace: &Path, max_mb: u64) -> Result<()> {
        let conn = self.conn.lock().expect("index lock");
        let cap = max_mb.saturating_mul(1024 * 1024);
        symbols::index_symbols(&conn, workspace, cap)?;
        text::index_chunks(&conn, workspace, cap)?;
        Ok(())
    }

    pub fn search_symbols(&self, query: &str, limit: usize) -> Result<Vec<SymbolHit>> {
        let conn = self.conn.lock().expect("index lock");
        symbols::search_symbols(&conn, query, limit)
    }

    pub fn search_semantic(&self, query: &str, limit: usize) -> Result<Vec<TextHit>> {
        let conn = self.conn.lock().expect("index lock");
        text::search_fts(&conn, query, limit)
    }
}

fn workspace_hash(workspace: &Path) -> String {
    let mut h = Sha256::new();
    h.update(workspace.to_string_lossy().as_bytes());
    format!("{:x}", h.finalize())
}
