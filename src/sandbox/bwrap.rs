//! Linux bubblewrap argument builder.

use std::path::Path;

/// Check if `bwrap` is on PATH.
pub fn is_available() -> bool {
    which::which("bwrap").is_ok()
}

/// Build a full argv for `bwrap … -- /bin/sh -c <command>`.
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
