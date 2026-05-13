use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub default_provider: String,
    pub agent: AgentConfig,
    pub providers: ProviderConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            default_provider: "ollama".to_owned(),
            agent: AgentConfig::default(),
            providers: ProviderConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    pub max_steps_per_turn: usize,
    pub max_patch_retries: usize,
    pub max_shell_retries: usize,
    pub max_tool_output_chars: usize,
    pub max_search_output_chars: usize,
    pub max_read_file_chars: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_steps_per_turn: 12,
            max_patch_retries: 2,
            max_shell_retries: 2,
            max_tool_output_chars: 30_000,
            max_search_output_chars: 20_000,
            max_read_file_chars: 16_000,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    pub ollama: OllamaConfig,
    pub openai_compat: OpenAiCompatConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct OllamaConfig {
    pub host: String,
    pub default_model: String,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            host: "http://localhost:11434".to_owned(),
            default_model: "qwen2.5-coder:7b".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct OpenAiCompatConfig {
    pub base_url: String,
    pub api_key_env: String,
    pub default_model: String,
}

impl Default for OpenAiCompatConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_owned(),
            api_key_env: "OPENAI_API_KEY".to_owned(),
            default_model: "gpt-4o-mini".to_owned(),
        }
    }
}
