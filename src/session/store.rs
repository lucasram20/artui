//! SQLite session store — WAL mode, ULID keys, write-through.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::util::time::now_iso;

/// A persisted session record.
#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub id: String,
    pub workspace: String,
    pub agent_id: String,
    pub created_at: String,
    pub updated_at: String,
}

/// A persisted message record.
#[derive(Debug, Clone)]
pub struct MessageRecord {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
    pub tool_call_id: Option<String>,
    pub finished_at: Option<String>,
}

/// A memory entry (project/user/session scoped).
#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub id: String,
    pub scope: String,
    pub key: String,
    pub value: String,
}

/// SQLite-backed session store.
pub struct SessionStore {
    conn: Connection,
    #[allow(dead_code)]
    db_path: PathBuf,
}

impl SessionStore {
    /// Open or create the database at the given path.
    /// Sets WAL mode and creates tables if needed.
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).context("failed to create database directory")?;
        }

        let conn = Connection::open(db_path).context("failed to open session database")?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )
        .context("failed to set database pragmas")?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = std::fs::set_permissions(db_path, perms);
        }

        conn.execute_batch(SCHEMA)
            .context("failed to initialize database schema")?;

        Ok(Self {
            conn,
            db_path: db_path.to_path_buf(),
        })
    }

    /// Open the default database location.
    pub fn open_default() -> Result<Self> {
        let data_dir = directories::ProjectDirs::from("", "", "artui")
            .context("cannot determine data directory")?
            .data_local_dir()
            .to_path_buf();
        Self::open(&data_dir.join("artui.db"))
    }

    // -----------------------------------------------------------------------
    // Sessions
    // -----------------------------------------------------------------------

    pub fn create_session(&self, workspace: &str, agent_id: &str) -> Result<SessionRecord> {
        let id = ulid::Ulid::new().to_string();
        let now = now_iso();
        self.conn.execute(
            "INSERT INTO sessions (id, workspace, agent_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, workspace, agent_id, now, now],
        ).context("failed to create session")?;
        Ok(SessionRecord {
            id,
            workspace: workspace.to_owned(),
            agent_id: agent_id.to_owned(),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn list_sessions(&self, workspace: &str, limit: usize) -> Result<Vec<SessionRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, workspace, agent_id, created_at, updated_at FROM sessions WHERE workspace = ?1 ORDER BY updated_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![workspace, limit as i64], |row| {
            Ok(SessionRecord {
                id: row.get(0)?,
                workspace: row.get(1)?,
                agent_id: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn touch_session(&self, session_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            params![now_iso(), session_id],
        )?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Messages
    // -----------------------------------------------------------------------

    pub fn append_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        tool_call_id: Option<&str>,
    ) -> Result<String> {
        let id = ulid::Ulid::new().to_string();
        let now = now_iso();
        self.conn.execute(
            "INSERT INTO messages (id, session_id, role, content, created_at, tool_call_id, finished_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, session_id, role, content, now, tool_call_id, Some(&now)],
        ).context("failed to append message")?;
        self.touch_session(session_id)?;
        Ok(id)
    }

    pub fn finish_message(&self, message_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET finished_at = ?1 WHERE id = ?2",
            params![now_iso(), message_id],
        )?;
        Ok(())
    }

    pub fn load_messages(&self, session_id: &str) -> Result<Vec<MessageRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, created_at, tool_call_id, finished_at FROM messages WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(MessageRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                created_at: row.get(4)?,
                tool_call_id: row.get(5)?,
                finished_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn flag_interrupted(&self, session_id: &str) -> Result<usize> {
        let count = self.conn.execute(
            "UPDATE messages SET content = content || '\n[interrupted]' WHERE session_id = ?1 AND finished_at IS NULL AND tool_call_id IS NOT NULL",
            params![session_id],
        )?;
        Ok(count)
    }

    // -----------------------------------------------------------------------
    // Memory
    // -----------------------------------------------------------------------

    pub fn memory_set(&self, scope: &str, key: &str, value: &str) -> Result<()> {
        let id = ulid::Ulid::new().to_string();
        self.conn.execute(
            "INSERT INTO memory (id, scope, key, value) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(scope, key) DO UPDATE SET value = excluded.value",
            params![id, scope, key, value],
        )?;
        Ok(())
    }

    pub fn memory_get(&self, scope: &str, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM memory WHERE scope = ?1 AND key = ?2")?;
        let mut rows = stmt.query(params![scope, key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn memory_list(&self, scope: &str) -> Result<Vec<MemoryEntry>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, scope, key, value FROM memory WHERE scope = ?1 ORDER BY key")?;
        let rows = stmt.query_map(params![scope], |row| {
            Ok(MemoryEntry {
                id: row.get(0)?,
                scope: row.get(1)?,
                key: row.get(2)?,
                value: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn memory_delete(&self, scope: &str, key: &str) -> Result<bool> {
        let count = self.conn.execute(
            "DELETE FROM memory WHERE scope = ?1 AND key = ?2",
            params![scope, key],
        )?;
        Ok(count > 0)
    }

    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM sessions WHERE id = ?1", params![session_id])?;
        Ok(())
    }
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    workspace TEXT NOT NULL,
    agent_id TEXT NOT NULL DEFAULT 'build',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL,
    tool_call_id TEXT,
    finished_at TEXT,
    compacted_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, created_at);

CREATE TABLE IF NOT EXISTS memory (
    id TEXT PRIMARY KEY,
    scope TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    UNIQUE(scope, key)
);

CREATE INDEX IF NOT EXISTS idx_memory_scope ON memory(scope);
";

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn create_and_load_session() {
        let dir = TempDir::new().unwrap();
        let store = SessionStore::open(&dir.path().join("test.db")).unwrap();

        let session = store.create_session("/tmp/project", "build").unwrap();
        assert!(!session.id.is_empty());

        store
            .append_message(&session.id, "user", "hello", None)
            .unwrap();
        store
            .append_message(&session.id, "assistant", "hi there", None)
            .unwrap();

        let messages = store.load_messages(&session.id).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "hello");
        assert_eq!(messages[1].role, "assistant");
    }

    #[test]
    fn list_sessions_by_workspace() {
        let dir = TempDir::new().unwrap();
        let store = SessionStore::open(&dir.path().join("test.db")).unwrap();

        store.create_session("/project/a", "build").unwrap();
        store.create_session("/project/a", "plan").unwrap();
        store.create_session("/project/b", "build").unwrap();

        let sessions = store.list_sessions("/project/a", 10).unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn memory_crud() {
        let dir = TempDir::new().unwrap();
        let store = SessionStore::open(&dir.path().join("test.db")).unwrap();

        store.memory_set("project:/tmp/x", "lang", "rust").unwrap();
        assert_eq!(
            store.memory_get("project:/tmp/x", "lang").unwrap(),
            Some("rust".to_owned())
        );

        store.memory_set("project:/tmp/x", "lang", "go").unwrap();
        assert_eq!(
            store.memory_get("project:/tmp/x", "lang").unwrap(),
            Some("go".to_owned())
        );

        let entries = store.memory_list("project:/tmp/x").unwrap();
        assert_eq!(entries.len(), 1);

        store.memory_delete("project:/tmp/x", "lang").unwrap();
        assert_eq!(store.memory_get("project:/tmp/x", "lang").unwrap(), None);
    }

    #[test]
    fn cascade_delete() {
        let dir = TempDir::new().unwrap();
        let store = SessionStore::open(&dir.path().join("test.db")).unwrap();

        let session = store.create_session("/tmp", "build").unwrap();
        store
            .append_message(&session.id, "user", "msg", None)
            .unwrap();

        store.delete_session(&session.id).unwrap();
        let messages = store.load_messages(&session.id).unwrap();
        assert!(messages.is_empty());
    }

    #[test]
    fn wal_mode_enabled() {
        let dir = TempDir::new().unwrap();
        let store = SessionStore::open(&dir.path().join("test.db")).unwrap();

        let mode: String = store
            .conn
            .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }
}
