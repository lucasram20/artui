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
    pub snapshots: SnapshotsConfig,
    pub sandbox: SandboxConfig,
    pub index: IndexConfig,
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
            lsp: LspConfig::default(),
            snapshots: SnapshotsConfig::default(),
            sandbox: SandboxConfig::default(),
            index: IndexConfig::default(),
        }
    }
}

/// Workspace symbol + text index (Phase M6).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct IndexConfig {
    pub enabled: bool,
    pub max_size_mb: u64,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_size_mb: 50,
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
    /// Provider id used when looking up credentials through
    /// [`crate::auth::resolve_credential`]. Defaults to `"openai_compat"`.
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
    /// Phase N3 — when true, every successful `apply_patch` runs a
    /// writethrough pass that pushes the new file contents to the LSP and
    /// waits for `publishDiagnostics`, then appends the diagnostics to the
    /// tool result so the model sees its own breakage immediately.
    pub writethrough: bool,
    /// Phase N3 — wall-clock budget for the post-apply_patch
    /// `publishDiagnostics` poll. Tight enough to keep the agent loop
    /// snappy, loose enough that rust-analyzer's incremental check has
    /// time to land. Default 750 ms.
    pub diagnostics_timeout_ms: u32,
}

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            warmup_on_startup: true,
            log_messages: false,
            request_timeout_secs: 10,
            writethrough: true,
            diagnostics_timeout_ms: 750,
        }
    }
}

/// `[snapshots]` — workspace snapshot & rollback safety net (Phase M3).
///
/// When `enabled = true` (default), artui keeps workspace snapshots under
/// `~/.local/share/artui/snapshots/<workspace_hash>/` and can auto-snapshot
/// before risky agent operations. `/snapshot list|restore <id>|clear` manage
/// them. Set `enabled = false` to disable the subsystem entirely.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SnapshotsConfig {
    /// Master switch. When false, no manager is constructed and no
    /// auto-snapshots fire.
    pub enabled: bool,
    /// Snapshot before each `apply_patch`.
    pub auto_pre_patch: bool,
    /// Snapshot before each non-read-only `shell` command.
    pub auto_pre_shell: bool,
    /// Snapshot once at the start of every agent turn.
    pub auto_per_turn: bool,
    /// Keep the newest N snapshots; older ones are auto-pruned.
    pub retain: usize,
    /// Skip a tar-backend snapshot when the workspace exceeds this many MB
    /// (uncompressed). Guards against archiving giant build dirs.
    pub max_tar_mb: u64,
}

impl Default for SnapshotsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_pre_patch: true,
            auto_pre_shell: true,
            auto_per_turn: false,
            retain: 20,
            max_tar_mb: 512,
        }
    }
}

/// `[sandbox]` — optional OS-level isolation for the `shell` tool (Phases J + M4).
///
/// `mode = "auto"` picks bubblewrap on Linux and Seatbelt (`sandbox-exec`) on macOS
/// when the binary is present. Missing backends fall back to unsandboxed execution
/// with a startup warning.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SandboxConfig {
    /// `off` | `auto` | `bubblewrap` | `seatbelt`
    pub mode: String,
    /// Allow outbound network inside the sandbox (default false).
    pub network: bool,
    /// Read-only access to `$HOME` (toolchain caches). Default false.
    pub allow_home_read: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            mode: "auto".to_owned(),
            network: false,
            allow_home_read: false,
        }
    }
}

impl SandboxConfig {
    /// Parsed mode for the sandbox module.
    pub fn mode(&self) -> crate::sandbox::SandboxMode {
        crate::sandbox::SandboxMode::parse_mode(&self.mode)
    }
}
