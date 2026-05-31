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
    pub lsp: LspConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            // freemodel routes through the artui Cloudflare Worker relay
            // (see `cloudflare/`), which holds the upstream API key
            // server-side, so the binary works without a `/login` step.
            // The user can still override `providers.freemodel.base_url`
            // and supply their own `FREEMODEL_API_KEY` to bypass the relay,
            // and they're free to switch to ollama or any other provider
            // through `/model`.
            default_provider: "freemodel".to_owned(),
            auth_storage_path: None,
            agent: AgentConfig::default(),
            providers: ProviderConfig::default(),
            ui: UiConfig::default(),
            updates: UpdateConfig::default(),
            permissions: PermissionsConfig::default(),
            lsp: LspConfig::default(),
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
    pub freemodel: FreemodelConfig,
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

/// Configuration for the freemodel.dev OpenAI-format gateway.
///
/// freemodel is registered as a no-login default provider. End-user binaries
/// route through the artui Cloudflare Worker relay (see `cloudflare/`),
/// which injects the upstream API key server-side — the binary itself ships
/// no credentials. The `default_model` is used as the active selection on
/// first launch and as the seed entry in the `/model` picker. `models` is
/// populated at runtime by hitting `GET {base_url}/models`; the configured
/// value is preserved so users can pin a specific list in
/// `~/.config/artui/config.toml` if they prefer.
///
/// Power users who want to bypass the relay and call freemodel.dev directly
/// can override `base_url` to `https://api.freemodel.dev/v1` in their
/// config and export `FREEMODEL_API_KEY` (or `ARTUI_FREEMODEL_API_KEY`).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct FreemodelConfig {
    pub base_url: String,
    pub default_model: String,
    /// Cached/configured model id list shown in the `/model` picker. Populated
    /// at startup by `GET {base_url}/models` and may be overridden in the
    /// user config file.
    pub models: Vec<String>,
    pub request_timeout_secs: u64,
}

impl Default for FreemodelConfig {
    fn default() -> Self {
        Self {
            // Default points at the artui Cloudflare Worker relay. The relay
            // injects the upstream API key, so the binary sends no
            // Authorization header (see `OpenAiCompatProvider::stream_chat`,
            // which conditionally adds the header only when
            // `resolve_credential` returns a value).
            //
            // ⚠ FORK MAINTAINERS: the `WORKERS_DEV_SUBDOMAIN_PLACEHOLDER`
            // segment below MUST be replaced with your actual workers.dev
            // subdomain after running `wrangler deploy` for the first time.
            // Wrangler prints the deployed URL on success, e.g.:
            //   Published artui-freemodel-relay (1.23 sec)
            //     https://artui-freemodel-relay.<your-subdomain>.workers.dev
            // Until you replace the placeholder, the binary will fail to
            // resolve the relay and chat will error out with a DNS failure.
            //
            // End users can also override `providers.freemodel.base_url` in
            // `~/.config/artui/config.toml` to point at any compatible
            // OpenAI-format gateway.
            base_url: "https://artui-freemodel-relay.<your-subdomain>.workers.dev/v1".to_owned(),
            default_model: "gpt-5.4-mini".to_owned(),
            models: Vec::new(),
            request_timeout_secs: 30,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct OpenAiCompatConfig {
    pub base_url: String,
    pub api_key_env: String,
    pub default_model: String,
    /// Provider id used when looking up credentials through
    /// [`crate::auth::resolve_credential`]. Defaults to `"openai_compat"` so
    /// existing user configs keep working; the freemodel provider sets this
    /// to `"freemodel"` so it picks up the freemodel-specific env vars and
    /// embedded fallback instead of OpenAI keys.
    #[serde(default = "default_credential_provider_id")]
    pub credential_provider_id: String,
}

fn default_credential_provider_id() -> String {
    "openai_compat".to_owned()
}

impl Default for OpenAiCompatConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_owned(),
            api_key_env: "OPENAI_API_KEY".to_owned(),
            default_model: "gpt-4o-mini".to_owned(),
            credential_provider_id: default_credential_provider_id(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct OpenAiAccountConfig {
    pub base_url: String,
    pub default_model: String,
    pub oauth_client_id: String,
    pub oauth_issuer: String,
    pub oauth_scope: String,
    pub oauth_port: u16,
    pub oauth_fallback_port: u16,
    pub oauth_timeout_secs: u64,
}

impl Default for OpenAiAccountConfig {
    fn default() -> Self {
        Self {
            base_url: "https://chatgpt.com/backend-api/codex".to_owned(),
            default_model: String::new(),
            oauth_client_id: String::new(),
            oauth_issuer: String::new(),
            oauth_scope: String::new(),
            oauth_port: 0,
            oauth_fallback_port: 0,
            oauth_timeout_secs: 0,
        }
    }
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
    /// When true, only show models GitHub explicitly marks
    /// `model_picker_enabled = true` (matches the VS Code Copilot Chat
    /// picker). When false (default), show every model the account is
    /// callable for and is not policy-disabled — student/free Copilot
    /// plans surface far more models this way.
    pub strict_picker: bool,
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
            strict_picker: false,
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

/// Configuration for the Language Server Protocol integration introduced in
/// Phase N1.
///
/// `enabled = true` is the default — the `lsp` tool registers and the agent
/// gets `definition` / `hover` / `status` actions out of the box (provided
/// the underlying language servers are on `$PATH`). Set `enabled = false`
/// to remove the tool from the registry entirely; the model never sees it.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LspConfig {
    /// Master switch.
    pub enabled: bool,
    /// When true, walk the workspace at startup and pre-spawn matching
    /// language servers so the first `definition` call is fast. Failures
    /// during warmup are logged and reported by `lsp status` but never
    /// block the agent loop.
    pub warmup_on_startup: bool,
    /// Forward `window/logMessage` notifications to `tracing` at INFO level
    /// instead of DEBUG. Useful when chasing server config issues.
    pub log_messages: bool,
    /// Per-request timeout (seconds). Override per-server via
    /// `~/.config/artui/lsp.toml` (Phase N2+).
    pub request_timeout_secs: u32,
}

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            warmup_on_startup: true,
            log_messages: false,
            request_timeout_secs: 10,
        }
    }
}
