//! FTS5 chunk search over indexed source lines.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};

#[derive(Debug, Clone)]
pub struct TextHit {
    pub path: String,
    pub line: u32,
    pub snippet: String,
}

/// Ensure `chunks` is an FTS5 virtual table (migrate legacy plain tables).
pub fn ensure_chunks_fts(conn: &Connection) -> Result<()> {
    let ddl: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type IN ('table','shadow') AND name = 'chunks'",
            [],
            |row| row.get(0),
        )
        .optional()
        .context("read chunks ddl")?;
    if ddl
        .as_deref()
        .is_some_and(|s| s.to_ascii_lowercase().contains("fts5"))
    {
        return Ok(());
    }
    conn.execute_batch("DROP TABLE IF EXISTS chunks;")?;
    conn.execute(
        "CREATE VIRTUAL TABLE chunks USING fts5(path UNINDEXED, line UNINDEXED, body)",
        [],
    )
    .context("create chunks fts5")?;
    Ok(())
}

pub fn index_chunks(conn: &Connection, workspace: &std::path::Path, cap_bytes: u64) -> Result<()> {
    use ignore::WalkBuilder;

    ensure_chunks_fts(conn)?;
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
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let fts_query = escape_fts_query(trimmed);
    let mut stmt =
        conn.prepare("SELECT path, line, body FROM chunks WHERE chunks MATCH ?1 LIMIT ?2")?;
    let rows = stmt.query_map(rusqlite::params![fts_query, limit as i64], |row| {
        Ok(TextHit {
            path: row.get(0)?,
            line: row.get::<_, i64>(1)? as u32,
            snippet: row.get(2)?,
        })
    });
    if let Ok(iter) = rows {
        let hits: Vec<_> = iter.filter_map(Result::ok).collect();
        if !hits.is_empty() {
            return Ok(hits);
        }
    }
    // Fallback for tokens FTS rejects (very short or special syntax).
    let pattern = format!("%{trimmed}%");
    let mut stmt =
        conn.prepare("SELECT path, line, body FROM chunks WHERE body LIKE ?1 LIMIT ?2")?;
    let rows = stmt.query_map(rusqlite::params![pattern, limit as i64], |row| {
        Ok(TextHit {
            path: row.get(0)?,
            line: row.get::<_, i64>(1)? as u32,
            snippet: row.get(2)?,
        })
    })?;
    Ok(rows.filter_map(Result::ok).collect())
}

/// Quote each whitespace-separated token for FTS5 phrase matching.
fn escape_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|token| {
            let escaped = token.replace('"', "\"\"");
            format!("\"{escaped}\"")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn fts5_match_beats_like_fallback() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_chunks_fts(&conn).unwrap();
        conn.execute(
            "INSERT INTO chunks (path, line, body) VALUES ('a.rs', 1, 'alpha beta gamma')",
            [],
        )
        .unwrap();
        let hits = search_fts(&conn, "beta", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.contains("beta"));
    }
}
