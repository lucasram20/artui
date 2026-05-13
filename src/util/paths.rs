use std::path::PathBuf;

use directories::ProjectDirs;

pub fn global_config_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "artui").map(|dirs| dirs.config_dir().join("config.toml"))
}
