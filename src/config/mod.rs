mod schema;

use anyhow::{Context, Result};

pub use schema::{
    AppConfig, CopilotConfig, IndexConfig, LspConfig, OllamaConfig, OpenAiAccountConfig,
    OpenAiCompatConfig, SandboxConfig, SnapshotsConfig, UpdateConfig,
};

pub fn load_global_config() -> Result<AppConfig> {
    let Some(path) = crate::util::paths::global_config_path() else {
        return Ok(AppConfig::default());
    };

    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read config at {}", path.display()))?;
    let config: AppConfig = toml::from_str(&content)
        .with_context(|| format!("failed to parse config at {}", path.display()))?;
    Ok(normalize_loaded_config(config))
}

/// Maps retired provider ids from older configs to a supported default.
fn normalize_loaded_config(mut config: AppConfig) -> AppConfig {
    if config.default_provider == "freemodel" {
        config.default_provider = "ollama".to_owned();
    }
    config
}
