//! User-defined lifecycle hooks fired on agent events.
//!
//! Modeled on Claude Code / pi-coding-agent extensions: declarative shell
//! commands attached to event types, configured via `.artui/hooks.json`
//! (project) or `~/.config/artui/hooks.json` (user).
//!
//! Format:
//! ```json
//! {
//!   "hooks": {
//!     "PreToolUse":  [{ "matcher": "shell|apply_patch", "command": "echo about-to-run" }],
//!     "PostToolUse": [{ "matcher": "*",                  "command": "echo done" }],
//!     "Stop":        [{ "matcher": "*",                  "command": "notify-send 'artui idle'" }]
//!   }
//! }
//! ```
//!
//! Matcher = pipe-separated glob list (`*` = any). Command runs through
//! `sh -c` on Unix / `cmd /C` on Windows with a 5 s default timeout. Failed
//! hooks are logged but never block the agent.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use tokio::process::Command;
use tokio::time::timeout;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct HookConfig {
    pub hooks: HashMap<HookEvent, Vec<HookEntry>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct HookEntry {
    /// Pipe-separated glob list. `*` matches anything. Empty string ≡ `*`.
    pub matcher: String,
    pub command: String,
    /// Working dir override. Falls back to workspace_root.
    pub cwd: Option<PathBuf>,
    /// Timeout in milliseconds. Default 5_000.
    pub timeout_ms: Option<u64>,
}

impl Default for HookEntry {
    fn default() -> Self {
        Self {
            matcher: "*".to_owned(),
            command: String::new(),
            cwd: None,
            timeout_ms: None,
        }
    }
}

/// Event kinds the agent loop fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum HookEvent {
    /// Fired before each `tools/dispatch` call.
    PreToolUse,
    /// Fired after every tool call regardless of success.
    PostToolUse,
    /// Fired when the agent loop exits (Done, error, or step limit).
    Stop,
    /// Fired when the user submits a new message.
    UserPrompt,
}

/// Discover hook config from project + user dirs. Project entries are
/// concatenated AFTER user entries so project hooks fire last.
pub fn load_hook_config(workspace_root: &Path) -> HookConfig {
    let mut merged = HookConfig::default();
    if let Some(user_path) = user_hook_path() {
        if let Some(cfg) = read_hook_file(&user_path) {
            for (event, entries) in cfg.hooks {
                merged.hooks.entry(event).or_default().extend(entries);
            }
        }
    }
    let project_path = workspace_root.join(".artui").join("hooks.json");
    if let Some(cfg) = read_hook_file(&project_path) {
        for (event, entries) in cfg.hooks {
            merged.hooks.entry(event).or_default().extend(entries);
        }
    }
    merged
}

fn user_hook_path() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.trim().is_empty() {
            return Some(PathBuf::from(xdg).join("artui").join("hooks.json"));
        }
    }
    let home = std::env::var("HOME").ok().filter(|h| !h.is_empty())?;
    Some(PathBuf::from(home).join(".config").join("artui").join("hooks.json"))
}

fn read_hook_file(path: &Path) -> Option<HookConfig> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Run every hook attached to `event` whose matcher accepts `target`.
/// `target` is typically a tool name (`shell`, `apply_patch`, …) or a
/// short label like `done`. Failures are logged via `tracing::warn` only.
pub async fn fire_hooks(
    cfg: &HookConfig,
    event: HookEvent,
    target: &str,
    workspace_root: &Path,
) {
    let Some(entries) = cfg.hooks.get(&event) else {
        return;
    };
    for entry in entries {
        if entry.command.trim().is_empty() {
            continue;
        }
        if !matches(&entry.matcher, target) {
            continue;
        }
        let cwd = entry.cwd.clone().unwrap_or_else(|| workspace_root.to_path_buf());
        let dur = Duration::from_millis(entry.timeout_ms.unwrap_or(5_000));

        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&entry.command);
            c
        };
        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = Command::new("sh");
            c.arg("-c").arg(&entry.command);
            c
        };
        cmd.current_dir(&cwd);
        cmd.env("ARTUI_HOOK_EVENT", format!("{event:?}"));
        cmd.env("ARTUI_HOOK_TARGET", target);
        cmd.kill_on_drop(true);

        let spawn = cmd.output();
        match timeout(dur, spawn).await {
            Ok(Ok(out)) if !out.status.success() => {
                tracing::warn!(
                    "hook '{}' for {:?}/{} exited {:?}",
                    entry.command,
                    event,
                    target,
                    out.status.code()
                );
            }
            Ok(Err(error)) => {
                tracing::warn!("hook '{}' failed to spawn: {error}", entry.command);
            }
            Err(_) => {
                tracing::warn!(
                    "hook '{}' for {:?}/{} timed out after {:?}",
                    entry.command,
                    event,
                    target,
                    dur
                );
            }
            _ => {}
        }
    }
}

fn matches(pattern: &str, target: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() || pattern == "*" {
        return true;
    }
    pattern
        .split('|')
        .map(str::trim)
        .any(|piece| piece == "*" || piece == target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_pattern_matches_anything() {
        assert!(matches("", "shell"));
        assert!(matches("*", "shell"));
    }

    #[test]
    fn pipe_separated_matchers() {
        assert!(matches("shell|apply_patch", "shell"));
        assert!(matches("shell|apply_patch", "apply_patch"));
        assert!(!matches("shell|apply_patch", "read_file"));
    }

    #[test]
    fn parses_minimal_hook_config() {
        let raw = r#"{
            "hooks": {
                "PreToolUse": [
                    { "matcher": "shell", "command": "echo before" }
                ],
                "Stop": [
                    { "matcher": "*", "command": "echo done", "timeout_ms": 1000 }
                ]
            }
        }"#;
        let cfg: HookConfig = serde_json::from_str(raw).unwrap();
        assert_eq!(cfg.hooks.get(&HookEvent::PreToolUse).unwrap().len(), 1);
        let stop = &cfg.hooks.get(&HookEvent::Stop).unwrap()[0];
        assert_eq!(stop.command, "echo done");
        assert_eq!(stop.timeout_ms, Some(1000));
    }

    #[tokio::test]
    async fn fire_hooks_runs_matching_command() {
        let tmp = tempfile::TempDir::new().unwrap();
        let stamp = tmp.path().join("stamp.txt");
        let mut hooks = HashMap::new();
        hooks.insert(
            HookEvent::PostToolUse,
            vec![HookEntry {
                matcher: "shell".to_owned(),
                command: format!("touch {}", stamp.display()),
                cwd: None,
                timeout_ms: Some(2_000),
            }],
        );
        let cfg = HookConfig { hooks };
        fire_hooks(&cfg, HookEvent::PostToolUse, "shell", tmp.path()).await;
        assert!(stamp.exists(), "hook should have created the stamp file");
    }

    #[tokio::test]
    async fn fire_hooks_skips_non_matching_target() {
        let tmp = tempfile::TempDir::new().unwrap();
        let stamp = tmp.path().join("nope.txt");
        let mut hooks = HashMap::new();
        hooks.insert(
            HookEvent::PostToolUse,
            vec![HookEntry {
                matcher: "apply_patch".to_owned(),
                command: format!("touch {}", stamp.display()),
                cwd: None,
                timeout_ms: Some(2_000),
            }],
        );
        let cfg = HookConfig { hooks };
        fire_hooks(&cfg, HookEvent::PostToolUse, "shell", tmp.path()).await;
        assert!(!stamp.exists(), "hook must not fire when target mismatches");
    }
}
