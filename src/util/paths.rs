use std::path::{Path, PathBuf};

use directories::{BaseDirs, ProjectDirs};

pub fn global_config_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "artui").map(|dirs| dirs.config_dir().join("config.toml"))
}

pub fn auth_store_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "artui").map(|dirs| dirs.data_dir().join("auth.json"))
}

/// Home directory for tilde shortening (`USERPROFILE` on Windows).
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            return Some(PathBuf::from(profile));
        }
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()))
}

/// Display path with `~` prefix when under the user's home directory.
pub fn compact_display_path(path: &Path) -> String {
    let Some(home) = home_dir() else {
        return path.display().to_string();
    };
    let Ok(rest) = path.strip_prefix(&home) else {
        return path.display().to_string();
    };
    let rest = rest.to_string_lossy();
    let rest = rest.trim_start_matches(['\\', '/']);
    if rest.is_empty() {
        return "~".to_owned();
    }

    #[cfg(windows)]
    {
        format!("~\\{rest}")
    }
    #[cfg(not(windows))]
    {
        format!("~/{rest}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[cfg(windows)]
    #[test]
    fn compact_display_path_shortens_under_home() {
        let home = Path::new(r"C:\Users\Romulus");
        let cwd = Path::new(r"C:\Users\Romulus\Desktop\Projects\artui");
        std::env::set_var("USERPROFILE", home);
        let compact = compact_display_path(cwd);
        assert_eq!(compact, r"~\Desktop\Projects\artui");
        std::env::remove_var("USERPROFILE");
    }

    #[cfg(windows)]
    #[test]
    fn compact_display_path_leaves_other_users_untouched() {
        let cwd = Path::new(r"C:\Users\OtherUser\Desktop");
        let compact = compact_display_path(cwd);
        assert_eq!(compact, r"C:\Users\OtherUser\Desktop");
    }

    #[cfg(not(windows))]
    #[test]
    fn compact_display_path_shortens_under_home() {
        let home = Path::new("/home/artui");
        let cwd = Path::new("/home/artui/projects/artui");
        std::env::set_var("HOME", home);
        let compact = compact_display_path(cwd);
        assert_eq!(compact, "~/projects/artui");
        std::env::remove_var("HOME");
    }
}
