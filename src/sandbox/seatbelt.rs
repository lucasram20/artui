//! macOS `sandbox-exec` (Seatbelt) profile builder and command wrapping.

use std::path::{Path, PathBuf};

/// Trusted path — do not resolve `sandbox-exec` from `$PATH`.
pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

const BASE_POLICY: &str = include_str!("seatbelt_base.sb");

/// Returns true when `/usr/bin/sandbox-exec` exists.
pub fn is_available() -> bool {
    path_is_executable(Path::new(SANDBOX_EXEC))
}

fn path_is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if meta.is_dir() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Build argv: `sandbox-exec -p <policy> -D… -- /bin/sh -c <command>`.
pub fn wrap_command(
    command: &str,
    cwd: &Path,
    workspace: &Path,
    network: bool,
    allow_home_read: bool,
) -> Vec<String> {
    let tmp = std::env::var("TMPDIR")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let home = std::env::var("HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);

    let (policy, params) = build_policy(workspace, &tmp, network, allow_home_read, home.as_deref());

    let mut argv = vec![SANDBOX_EXEC.to_owned(), "-p".to_owned(), policy];
    for (key, value) in params {
        argv.push(format!("-D{key}={}", value.to_string_lossy()));
    }
    argv.push("--".to_owned());
    argv.push("/bin/sh".to_owned());
    argv.push("-c".to_owned());
    argv.push(command.to_owned());
    // sandbox-exec runs with cwd inherited; chdir inside profile is workspace-rooted
    let _ = cwd;
    argv
}

fn build_policy(
    workspace: &Path,
    tmp: &Path,
    network: bool,
    allow_home_read: bool,
    home: Option<&Path>,
) -> (String, Vec<(String, PathBuf)>) {
    let mut params = vec![
        ("WORKSPACE".to_owned(), workspace.to_path_buf()),
        ("TMP".to_owned(), tmp.to_path_buf()),
    ];

    let mut dynamic = String::new();
    dynamic.push_str("; workspace + temp (read/write)\n");
    dynamic.push_str("(allow file-read* file-write* (subpath (param \"WORKSPACE\")))\n");
    dynamic.push_str("(allow file-read* file-write* (subpath (param \"TMP\")))\n");

    dynamic.push_str("; read-only system roots\n");
    for root in ["/usr", "/System", "/Library", "/private/etc"] {
        dynamic.push_str(&format!("(allow file-read* (literal \"{root}\"))\n"));
    }

    if allow_home_read {
        if let Some(home) = home {
            params.push(("HOME".to_owned(), home.to_path_buf()));
            dynamic.push_str("(allow file-read* (subpath (param \"HOME\")))\n");
        }
    }

    if network {
        dynamic.push_str("(allow network-outbound)\n(allow network-inbound)\n");
    }

    let policy = format!("{BASE_POLICY}\n{dynamic}");
    (policy, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn wrap_command_starts_with_sandbox_exec() {
        let args = wrap_command(
            "echo hi",
            &PathBuf::from("/tmp/ws"),
            &PathBuf::from("/tmp/ws"),
            false,
            false,
        );
        assert_eq!(args[0], SANDBOX_EXEC);
        assert_eq!(args[1], "-p");
        assert!(args.contains(&"--".to_owned()));
        assert!(args.contains(&"echo hi".to_owned()));
    }

    #[test]
    fn policy_includes_workspace_param() {
        let (policy, params) =
            build_policy(Path::new("/proj"), Path::new("/tmp"), false, false, None);
        assert!(policy.contains("(param \"WORKSPACE\")"));
        assert!(params.iter().any(|(k, _)| k == "WORKSPACE"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sandbox_exec_available_on_macos_ci() {
        // Many CI macOS images ship sandbox-exec; skip assertion when absent.
        if is_available() {
            let ws = std::env::temp_dir().join("artui_seatbelt_test");
            let _ = std::fs::create_dir_all(&ws);
            let args = wrap_command("echo ok", &ws, &ws, false, false);
            let status = std::process::Command::new(&args[0])
                .args(&args[1..])
                .status()
                .expect("spawn sandbox-exec");
            assert!(status.success(), "seatbelt smoke test failed");
        }
    }
}
