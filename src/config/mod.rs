mod schema;

use anyhow::{Context, Result};

pub use schema::{
    AppConfig, CopilotConfig, FreemodelConfig, IndexConfig, LspConfig, OllamaConfig,
    OpenAiAccountConfig, OpenAiCompatConfig, SandboxConfig, SnapshotsConfig, UpdateConfig,
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
    toml::from_str(&content)
        .with_context(|| format!("failed to parse config at {}", path.display()))
}
