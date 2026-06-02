//! FTS5 chunk search over indexed source lines.

use anyhow::Result;
use rusqlite::Connection;

#[derive(Debug, Clone)]
pub struct TextHit {
    pub path: String,
    pub line: u32,
    pub snippet: String,
}

pub fn index_chunks(conn: &Connection, workspace: &std::path::Path, cap_bytes: u64) -> Result<()> {
    use ignore::WalkBuilder;

    conn.execute("DELETE FROM chunks", [])?;
    let mut total = 0u64;
    for entry in WalkBuilder::new(workspace)
        .hidden(false)
        .git_ignore(true)
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
    {
        let path = entry.path();
        if !super::symbols::is_indexable(path) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        total += content.len() as u64;
        if total > cap_bytes {
            break;
        }
        let rel = path
            .strip_prefix(workspace)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        for (i, line) in content.lines().enumerate() {
            conn.execute(
                "INSERT INTO chunks (path, line, body) VALUES (?1, ?2, ?3)",
                rusqlite::params![rel, (i + 1) as i64, line],
            )?;
        }
    }
    Ok(())
}

pub fn search_fts(conn: &Connection, query: &str, limit: usize) -> Result<Vec<TextHit>> {
    let mut stmt = conn.prepare(
        "SELECT path, line, body FROM chunks WHERE body MATCH ?1 LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![query, limit as i64], |row| {
        Ok(TextHit {
            path: row.get(0)?,
            line: row.get::<_, i64>(1)? as u32,
            snippet: row.get(2)?,
        })
    });
    match rows {
        Ok(iter) => Ok(iter.filter_map(Result::ok).collect()),
        Err(_) => {
            // Fallback: LIKE when MATCH syntax fails (e.g. short tokens)
            let pattern = format!("%{query}%");
            let mut stmt = conn.prepare(
                "SELECT path, line, body FROM chunks WHERE body LIKE ?1 LIMIT ?2",
            )?;
            let rows = stmt.query_map(rusqlite::params![pattern, limit as i64], |row| {
                Ok(TextHit {
                    path: row.get(0)?,
                    line: row.get::<_, i64>(1)? as u32,
                    snippet: row.get(2)?,
                })
            })?;
            Ok(rows.filter_map(Result::ok).collect())
        }
    }
}