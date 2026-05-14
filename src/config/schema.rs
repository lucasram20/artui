use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub default_provider: String,
    pub agent: AgentConfig,
    pub providers: ProviderConfig,
    pub ui: UiConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            default_provider: "ollama".to_owned(),
            agent: AgentConfig::default(),
            providers: ProviderConfig::default(),
            ui: UiConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub thinking_phrases: Vec<String>,
    pub reasoning_phrases: Vec<String>,
    pub reasoning_model_patterns: Vec<String>,
    pub spinner_frames: Vec<String>,
    pub spinner_interval_ms: u64,
    pub phrase_interval_ms: u64,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            thinking_phrases: default_thinking_phrases(),
            reasoning_phrases: default_reasoning_phrases(),
            reasoning_model_patterns: Vec::new(),
            spinner_frames: default_spinner_frames(),
            spinner_interval_ms: 120,
            phrase_interval_ms: 1_800,
        }
    }
}

fn default_thinking_phrases() -> Vec<String> {
    [
        "Working",
        "Reading",
        "Mapping",
        "Planning",
        "Checking",
        "Composing",
        "Stitching",
        "Polishing",
        "Untangling",
        "Brewing",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn default_reasoning_phrases() -> Vec<String> {
    [
        "Thinking",
        "Reasoning",
        "Weighing options",
        "Tracing logic",
        "Checking assumptions",
        "Connecting clues",
        "Exploring paths",
        "Refining plan",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn default_spinner_frames() -> Vec<String> {
    ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
        .into_iter()
        .map(str::to_owned)
        .collect()
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
            default_model: "gemma4:e2b".to_owned(),
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
