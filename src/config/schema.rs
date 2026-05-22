use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub default_provider: String,
    pub auth_storage_path: Option<PathBuf>,
    pub agent: AgentConfig,
    pub providers: ProviderConfig,
    pub ui: UiConfig,
    pub updates: UpdateConfig,
    pub permissions: PermissionsConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            default_provider: "ollama".to_owned(),
            auth_storage_path: None,
            agent: AgentConfig::default(),
            providers: ProviderConfig::default(),
            ui: UiConfig::default(),
            updates: UpdateConfig::default(),
            permissions: PermissionsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    /// Repo to poll for new releases (`owner/name`).
    pub repo: String,
    /// Severity threshold that surfaces an update banner. Defaults to
    /// `major` so day-to-day patch/feature releases stay quiet.
    pub notify_level: crate::update::NotifyLevel,
    /// When false, artui never reaches out to GitHub at startup.
    pub auto_check: bool,
    /// HTTP timeout for the `releases/latest` call (seconds).
    pub timeout_secs: u64,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            repo: "lucasram20/artui".to_owned(),
            notify_level: crate::update::NotifyLevel::Major,
            auto_check: true,
            timeout_secs: 5,
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
    /// Auto-compaction toggle. When false, the agent loop never compacts.
    pub compaction_auto: bool,
    /// Reserved output budget (tokens). Compaction triggers when
    /// `estimated_tokens >= context_window - compaction_reserve_tokens`.
    /// Mirrors opencode's `COMPACTION_BUFFER` (20k) and pi's `reserveTokens` (16k).
    pub compaction_reserve_tokens: u32,
    /// How many recent message tokens to preserve verbatim during compaction.
    pub compaction_keep_recent_tokens: u32,
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
            compaction_auto: true,
            compaction_reserve_tokens: 20_000,
            compaction_keep_recent_tokens: 8_000,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    pub ollama: OllamaConfig,
    pub openai_compat: OpenAiCompatConfig,
    pub openai_account: OpenAiAccountConfig,
    pub copilot: CopilotConfig,
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

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct OpenAiAccountConfig {
    pub base_url: String,
    pub default_model: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CopilotConfig {
    pub api_base_url: String,
    pub token_url: String,
    pub models_url: String,
    pub models: Vec<String>,
    pub integration_id: String,
    pub editor_version: String,
    pub editor_plugin_version: String,
    pub github_oauth_client_id: String,
    pub github_device_code_url: String,
    pub github_oauth_token_url: String,
    pub github_oauth_scope: String,
    pub github_login_timeout_secs: u64,
    pub github_token_env: Vec<String>,
    pub request_timeout_secs: u64,
    pub default_model: String,
}

impl Default for CopilotConfig {
    fn default() -> Self {
        Self {
            api_base_url: "https://api.githubcopilot.com".to_owned(),
            token_url: "https://api.github.com/copilot_internal/v2/token".to_owned(),
            models_url: "https://api.githubcopilot.com/models".to_owned(),
            models: Vec::new(),
            integration_id: "vscode-chat".to_owned(),
            editor_version: "vscode/1.99.2".to_owned(),
            editor_plugin_version: "copilot-chat/0.26.3".to_owned(),
            github_oauth_client_id: "Ov23liSsh5cnZv6yAz4X".to_owned(),
            github_device_code_url: "https://github.com/login/device/code".to_owned(),
            github_oauth_token_url: "https://github.com/login/oauth/access_token".to_owned(),
            github_oauth_scope: "read:user".to_owned(),
            github_login_timeout_secs: 900,
            github_token_env: vec!["GITHUB_TOKEN".to_owned(), "GH_TOKEN".to_owned()],
            request_timeout_secs: 30,
            default_model: String::new(),
        }
    }
}

/// Per-tool permission overrides loaded from `[permissions]` in
/// `~/.config/artui/config.toml`.
///
/// Default policy (engine-side, no override needed):
/// - `read_file` / `glob` / `search` → `allow`
/// - `apply_patch` / `shell` → `ask` in Build, `deny` in Plan
/// - unknown tools → `ask`
///
/// Example overrides:
/// ```toml
/// [permissions.tools]
/// apply_patch = "allow"     # opt back into auto-allow
/// shell       = "deny"      # never run shell, even in Build
/// network     = "ask"       # gate a future web tool
/// ```
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PermissionsConfig {
    /// `tool_name` → `"allow" | "ask" | "deny"`. Anything unrecognised is
    /// dropped silently by the engine, falling back to the default policy.
    pub tools: std::collections::HashMap<String, String>,
}
