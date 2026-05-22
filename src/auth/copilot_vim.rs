//! Reader for `~/.config/github-copilot/hosts.json` and `apps.json`.
//!
//! Both files are written by the official `github/copilot.vim` plugin and the
//! VS Code Copilot extension. We surface their stored OAuth tokens as a third
//! source so users authenticated via gh-CLI / Copilot.vim get zero-login.
//!
//! Format references:
//! - `hosts.json` (legacy): `{"github.com": {"user": "...", "oauth_token": "gho_..."}}`
//! - `apps.json` (newer):   `{"github.com:Iv1.<client_id>": {"user": "...", "oauth_token": "gho_..."}}`
//!
//! Tokens are read-only; we never write to these files.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct CopilotVimToken {
    pub token: String,
    pub host: String,
    pub source: String,
}

#[derive(Debug, Deserialize)]
struct HostEntry {
    #[serde(default)]
    oauth_token: Option<String>,
    #[serde(default)]
    user: Option<String>,
}

/// Discover OAuth tokens written by github/copilot.vim or the VS Code Copilot
/// extension. Returns an empty vec on any failure (missing dir, bad JSON, etc).
pub fn read_copilot_vim_tokens() -> Vec<CopilotVimToken> {
    let mut out = Vec::new();
    let Some(dir) = copilot_vim_config_dir() else {
        return out;
    };
    extend_from_file(&mut out, &dir.join("hosts.json"), "copilot.vim hosts.json");
    extend_from_file(&mut out, &dir.join("apps.json"), "copilot.vim apps.json");
    out
}

fn copilot_vim_config_dir() -> Option<PathBuf> {
    // Honor $XDG_CONFIG_HOME first, then fall back to $HOME/.config.
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.trim().is_empty() {
            return Some(PathBuf::from(xdg).join("github-copilot"));
        }
    }
    let home = std::env::var("HOME").ok().filter(|h| !h.is_empty())?;
    Some(PathBuf::from(home).join(".config").join("github-copilot"))
}

fn extend_from_file(out: &mut Vec<CopilotVimToken>, path: &std::path::Path, label: &str) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(parsed) = serde_json::from_str::<BTreeMap<String, HostEntry>>(&raw) else {
        return;
    };
    for (key, entry) in parsed {
        let Some(token) = entry.oauth_token.filter(|t| !t.trim().is_empty()) else {
            continue;
        };
        // hosts.json keys are bare hosts ("github.com"); apps.json keys are
        // "host:client_id". Normalise to the host portion for downstream URLs.
        let host = key.split(':').next().unwrap_or(&key).to_owned();
        let user_label = entry
            .user
            .as_deref()
            .map(|u| format!(" ({u})"))
            .unwrap_or_default();
        out.push(CopilotVimToken {
            token,
            host,
            source: format!("{label}{user_label}"),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;
    use tempfile::TempDir;

    // Tests mutate global env (XDG_CONFIG_HOME) — serialize them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_xdg<F: FnOnce()>(dir: &std::path::Path, f: F) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let prev_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        let prev_home = std::env::var("HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", dir);
        // Also override HOME so the fallback path inside copilot_vim_config_dir()
        // never resolves to the developer's real $HOME/.config.
        std::env::set_var("HOME", dir);
        f();
        match prev_xdg {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        match prev_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn returns_empty_when_no_files() {
        let tmp = TempDir::new().unwrap();
        with_xdg(tmp.path(), || {
            assert!(read_copilot_vim_tokens().is_empty());
        });
    }

    #[test]
    fn reads_hosts_json_legacy_format() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("github-copilot");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("hosts.json"),
            r#"{"github.com":{"user":"alice","oauth_token":"gho_aaaaaaaaaaaa"}}"#,
        )
        .unwrap();

        with_xdg(tmp.path(), || {
            let tokens = read_copilot_vim_tokens();
            assert_eq!(tokens.len(), 1);
            assert_eq!(tokens[0].token, "gho_aaaaaaaaaaaa");
            assert_eq!(tokens[0].host, "github.com");
            assert!(tokens[0].source.contains("hosts.json"));
            assert!(tokens[0].source.contains("alice"));
        });
    }

    #[test]
    fn reads_apps_json_namespaced_keys() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("github-copilot");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("apps.json"),
            r#"{"github.com:Iv1.b507a08c87ecfe98":{"user":"bob","oauth_token":"gho_bbbbbbbbbbbb"}}"#,
        )
        .unwrap();

        with_xdg(tmp.path(), || {
            let tokens = read_copilot_vim_tokens();
            assert_eq!(tokens.len(), 1);
            assert_eq!(tokens[0].token, "gho_bbbbbbbbbbbb");
            assert_eq!(tokens[0].host, "github.com");
            assert!(tokens[0].source.contains("apps.json"));
        });
    }

    #[test]
    fn skips_empty_tokens() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("github-copilot");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("hosts.json"),
            r#"{"github.com":{"user":"x","oauth_token":""}}"#,
        )
        .unwrap();

        with_xdg(tmp.path(), || {
            assert!(read_copilot_vim_tokens().is_empty());
        });
    }

    #[test]
    fn ignores_malformed_json() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("github-copilot");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("hosts.json"), "not-json").unwrap();
        with_xdg(tmp.path(), || {
            assert!(read_copilot_vim_tokens().is_empty());
        });
    }
}
