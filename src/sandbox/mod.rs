//! Linux bubblewrap sandbox for shell tool execution.
//!
//! When `bwrap` is available and sandbox mode is enabled, shell commands
//! run inside a restricted namespace with read-only system mounts and
//! writable workspace.

use std::path::Path;

/// Check if bwrap is available on the system.
pub fn is_available() -> bool {
    which::which("bwrap").is_ok()
}

/// Build bwrap command arguments for sandboxed execution.
///
/// The sandbox provides:
/// - Read-only bind mounts for /usr, /lib, /lib64, /bin, /sbin, /etc
/// - Writable bind mount for the workspace
/// - /proc, /dev, /tmp available
/// - Optional network isolation (--unshare-net)
/// - die-with-parent to prevent orphan processes
pub fn wrap_command(command: &str, cwd: &Path, workspace: &Path, network: bool) -> Vec<String> {
    let workspace_str = workspace.to_string_lossy();
    let cwd_str = cwd.to_string_lossy();

    let mut args: Vec<String> = vec![
        "bwrap".to_owned(),
        "--ro-bind".to_owned(),
        "/usr".to_owned(),
        "/usr".to_owned(),
        "--ro-bind".to_owned(),
        "/bin".to_owned(),
        "/bin".to_owned(),
    ];

    if Path::new("/lib").exists() {
        args.extend(["--ro-bind".to_owned(), "/lib".to_owned(), "/lib".to_owned()]);
    }
    if Path::new("/lib64").exists() {
        args.extend([
            "--ro-bind".to_owned(),
            "/lib64".to_owned(),
            "/lib64".to_owned(),
        ]);
    }
    if Path::new("/sbin").exists() {
        args.extend([
            "--ro-bind".to_owned(),
            "/sbin".to_owned(),
            "/sbin".to_owned(),
        ]);
    }

    args.extend(["--ro-bind".to_owned(), "/etc".to_owned(), "/etc".to_owned()]);

    args.extend([
        "--proc".to_owned(),
        "/proc".to_owned(),
        "--dev".to_owned(),
        "/dev".to_owned(),
        "--tmpfs".to_owned(),
        "/tmp".to_owned(),
    ]);

    args.extend([
        "--bind".to_owned(),
        workspace_str.to_string(),
        workspace_str.to_string(),
    ]);

    args.extend(["--chdir".to_owned(), cwd_str.to_string()]);

    args.extend(["--die-with-parent".to_owned(), "--new-session".to_owned()]);

    if !network {
        args.push("--unshare-net".to_owned());
    }

    args.extend([
        "--".to_owned(),
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        command.to_owned(),
    ]);

    args
}

/// Configuration for sandbox behavior.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    pub enabled: bool,
    pub network: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            network: false,
        }
    }
}

impl SandboxConfig {
    /// Effective sandbox: enabled only if configured AND bwrap available.
    pub fn is_active(&self) -> bool {
        self.enabled && is_available()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn wrap_command_structure() {
        let args = wrap_command(
            "cargo test",
            &PathBuf::from("/home/user/project"),
            &PathBuf::from("/home/user/project"),
            false,
        );

        assert_eq!(args[0], "bwrap");
        assert!(args.contains(&"--ro-bind".to_owned()));
        assert!(args.contains(&"--die-with-parent".to_owned()));
        assert!(args.contains(&"--new-session".to_owned()));
        assert!(args.contains(&"--unshare-net".to_owned()));
        assert!(args.contains(&"cargo test".to_owned()));
    }

    #[test]
    fn wrap_command_with_network() {
        let args = wrap_command(
            "curl example.com",
            &PathBuf::from("/tmp/ws"),
            &PathBuf::from("/tmp/ws"),
            true,
        );

        assert!(!args.contains(&"--unshare-net".to_owned()));
    }

    #[test]
    fn sandbox_config_inactive_when_disabled() {
        let config = SandboxConfig {
            enabled: false,
            network: false,
        };
        assert!(!config.is_active());
    }
}
