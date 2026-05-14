use std::path::PathBuf;

use directories::ProjectDirs;

pub fn global_config_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "artui").map(|dirs| dirs.config_dir().join("config.toml"))
}

pub fn auth_store_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "artui").map(|dirs| dirs.data_dir().join("auth.json"))
}
