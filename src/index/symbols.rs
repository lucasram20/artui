//! Lightweight symbol index (regex/line-based, no tree-sitter yet).

use std::path::Path;

use anyhow::Result;
use ignore::WalkBuilder;
use rusqlite::Connection;

#[derive(Debug, Clone)]
pub struct SymbolHit {
    pub path: String,
    pub name: String,
    pub line: u32,
}

const EXT: &[&str] = &["rs", "py", "ts", "tsx", "js", "jsx", "go"];

pub fn is_indexable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| EXT.contains(&e))
        .unwrap_or(false)
}

pub fn index_symbols(conn: &Connection, workspace: &Path, cap_bytes: u64) -> Result<()> {
    conn.execute("DELETE FROM symbols", [])?;
    let mut total = 0u64;
    for entry in WalkBuilder::new(workspace)
        .hidden(false)
        .git_ignore(true)
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
    {
        let path = entry.path();
        if !is_indexable(path) {
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
            if let Some(name) = extract_symbol_name(line) {
                conn.execute(
                    "INSERT INTO symbols (path, name, line) VALUES (?1, ?2, ?3)",
                    rusqlite::params![rel, name, (i + 1) as i64],
                )?;
            }
        }
    }
    Ok(())
}

pub fn search_symbols(conn: &Connection, query: &str, limit: usize) -> Result<Vec<SymbolHit>> {
    let pattern = format!("%{query}%");
    let mut stmt = conn.prepare(
        "SELECT path, name, line FROM symbols WHERE name LIKE ?1 OR path LIKE ?1 LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![pattern, limit as i64], |row| {
        Ok(SymbolHit {
            path: row.get(0)?,
            name: row.get(1)?,
            line: row.get::<_, i64>(2)? as u32,
        })
    })?;
    Ok(rows.filter_map(Result::ok).collect())
}

fn extract_symbol_name(line: &str) -> Option<String> {
    let trimmed = line.trim();
    for prefix in ["pub fn ", "fn ", "def ", "class ", "pub struct ", "struct "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}
