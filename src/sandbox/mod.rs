//! Platform sandboxes for shell tool execution.
//!
//! Linux and macOS use bubblewrap (`bwrap`) when available. macOS can opt into
//! Seatbelt via `mode = "seatbelt"`. Windows uses a Job Object backend (M5).
//! When the configured backend is unavailable, commands run unsandboxed and
//! a startup warning is logged.

pub mod bwrap;

#[cfg(target_os = "macos")]
pub mod seatbelt;

#[cfg(windows)]
pub mod win_jobobject;

use std::path::Path;

use crate::config::SandboxConfig;

/// How sandboxing is selected from config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxMode {
    Off,
    Auto,
    Bubblewrap,
    Seatbelt,
    WinJob,
}

impl SandboxMode {
    pub fn parse_mode(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "disabled" => Self::Off,
            "bubblewrap" | "bwrap" => Self::Bubblewrap,
            "seatbelt" | "sandbox-exec" | "sandbox_exec" => Self::Seatbelt,
            "win_job" | "windows" | "jobobject" => Self::WinJob,
            _ => Self::Auto,
        }
    }
}

/// Active isolation backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxBackend {
    Bubblewrap,
    #[cfg(target_os = "macos")]
    Seatbelt,
    #[cfg(windows)]
    WinJob,
}

/// Resolved sandbox settings threaded through `ToolContext`.
#[derive(Debug, Clone)]
pub struct SandboxSettings {
    pub mode: SandboxMode,
    pub network: bool,
    pub allow_home_read: bool,
    backend: Option<SandboxBackend>,
}

impl Default for SandboxSettings {
    fn default() -> Self {
        Self {
            mode: SandboxMode::Off,
            network: false,
            allow_home_read: false,
            backend: None,
        }
    }
}

impl SandboxSettings {
    pub fn from_config(cfg: &SandboxConfig) -> Self {
        let mode = cfg.mode();
        let backend = resolve_backend(mode);
        Self {
            mode,
            network: cfg.network,
            allow_home_read: cfg.allow_home_read,
            backend,
        }
    }

    /// True when a backend is resolved and the tool should wrap shell commands.
    pub fn is_active(&self) -> bool {
        self.backend.is_some()
    }

    pub fn backend(&self) -> Option<SandboxBackend> {
        self.backend
    }

    /// User-visible status for startup logging.
    pub fn startup_message(&self) -> Option<&'static str> {
        match (self.mode, self.backend) {
            (SandboxMode::Off, _) => None,
            (_, Some(_)) => None,
            (SandboxMode::Auto, None) => Some(
                "sandbox: auto mode enabled but no backend found (install bwrap on Linux/macOS, or use Windows 10+); shell runs unsandboxed",
            ),
            #[cfg(windows)]
            (SandboxMode::WinJob, None) => Some(
                "sandbox: win_job mode requires Windows 10+; shell runs unsandboxed",
            ),
            #[cfg(not(windows))]
            (SandboxMode::WinJob, None) => Some(
                "sandbox: win_job mode only applies on Windows; shell runs unsandboxed",
            ),
            (SandboxMode::Bubblewrap, None) => {
                Some("sandbox: bubblewrap mode enabled but `bwrap` not found; shell runs unsandboxed")
            }
            #[cfg(target_os = "macos")]
            (SandboxMode::Seatbelt, None) => Some(
                "sandbox: seatbelt mode enabled but /usr/bin/sandbox-exec missing; shell runs unsandboxed",
            ),
            #[cfg(not(target_os = "macos"))]
            (SandboxMode::Seatbelt, None) => Some(
                "sandbox: seatbelt mode only applies on macOS; shell runs unsandboxed",
            ),
        }
    }

    /// Build argv for a shell command (`argv[0]` is the program to spawn).
    pub fn wrap_shell_command(
        &self,
        command: &str,
        cwd: &Path,
        workspace: &Path,
    ) -> Option<Vec<String>> {
        let backend = self.backend?;
        Some(match backend {
            SandboxBackend::Bubblewrap => {
                bwrap::wrap_command(command, cwd, workspace, self.network)
            }
            #[cfg(target_os = "macos")]
            SandboxBackend::Seatbelt => {
                seatbelt::wrap_command(command, cwd, workspace, self.network, self.allow_home_read)
            }
            #[cfg(windows)]
            SandboxBackend::WinJob => {
                win_jobobject::wrap_command(command, cwd, workspace, self.network)
            }
        })
    }
}

fn resolve_backend(mode: SandboxMode) -> Option<SandboxBackend> {
    match mode {
        SandboxMode::Off => None,
        SandboxMode::Auto => {
            #[cfg(windows)]
            {
                if win_jobobject::is_available() {
                    return Some(SandboxBackend::WinJob);
                }
                None
            }
            #[cfg(not(windows))]
            {
                if bwrap::is_available() {
                    Some(SandboxBackend::Bubblewrap)
                } else {
                    None
                }
            }
        }
        SandboxMode::WinJob => {
            #[cfg(windows)]
            {
                if win_jobobject::is_available() {
                    Some(SandboxBackend::WinJob)
                } else {
                    None
                }
            }
            #[cfg(not(windows))]
            {
                None
            }
        }
        SandboxMode::Bubblewrap => {
            if bwrap::is_available() {
                Some(SandboxBackend::Bubblewrap)
            } else {
                None
            }
        }
        SandboxMode::Seatbelt => {
            #[cfg(target_os = "macos")]
            {
                if seatbelt::is_available() {
                    Some(SandboxBackend::Seatbelt)
                } else {
                    None
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn off_mode_inactive() {
        let s = SandboxSettings {
            mode: SandboxMode::Off,
            network: false,
            allow_home_read: false,
            backend: None,
        };
        assert!(!s.is_active());
    }

    #[test]
    fn wrap_command_structure_linux() {
        if !bwrap::is_available() {
            return;
        }
        let s = SandboxSettings {
            mode: SandboxMode::Bubblewrap,
            network: false,
            allow_home_read: false,
            backend: Some(SandboxBackend::Bubblewrap),
        };
        let args = s
            .wrap_shell_command(
                "cargo test",
                &PathBuf::from("/home/user/project"),
                &PathBuf::from("/home/user/project"),
            )
            .unwrap();
        assert_eq!(args[0], "bwrap");
        assert!(args.contains(&"--unshare-net".to_owned()));
        assert!(args.contains(&"cargo test".to_owned()));
    }

    #[test]
    fn wrap_command_with_network() {
        if !bwrap::is_available() {
            return;
        }
        let s = SandboxSettings {
            mode: SandboxMode::Bubblewrap,
            network: true,
            allow_home_read: false,
            backend: Some(SandboxBackend::Bubblewrap),
        };
        let args = s
            .wrap_shell_command(
                "curl example.com",
                &PathBuf::from("/tmp/ws"),
                &PathBuf::from("/tmp/ws"),
            )
            .unwrap();
        assert!(!args.contains(&"--unshare-net".to_owned()));
    }
}
