use std::{
    process::Command,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    agent::PrimaryAgent,
    auth::{AuthRecord, AuthStatus, AuthStore, GitHubDeviceFlowConfig},
    config::{AppConfig, CopilotConfig},
    providers::{
        build_provider,
        registry::{self, AuthRequirement, PROVIDERS},
        LlmProvider, ModelEvent, ModelRequest,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    Normal,
    Input,
    Streaming,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    MonokaiBlue,
    TokyoNight,
    CatppuccinMocha,
    Gruvbox,
    Nord,
    Dracula,
    Aura,
    SolarizedDark,
    OceanicNext,
    RosePine,
    Everforest,
    Kanagawa,
    AyuMirage,
    NightOwl,
}

impl ThemeId {
    pub const ALL: [Self; 14] = [
        Self::MonokaiBlue,
        Self::TokyoNight,
        Self::CatppuccinMocha,
        Self::Gruvbox,
        Self::Nord,
        Self::Dracula,
        Self::Aura,
        Self::SolarizedDark,
        Self::OceanicNext,
        Self::RosePine,
        Self::Everforest,
        Self::Kanagawa,
        Self::AyuMirage,
        Self::NightOwl,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::MonokaiBlue => "Monokai Blue",
            Self::TokyoNight => "Tokyo Night",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::Gruvbox => "Gruvbox",
            Self::Nord => "Nord",
            Self::Dracula => "Dracula",
            Self::Aura => "Aura",
            Self::SolarizedDark => "Solarized Dark",
            Self::OceanicNext => "Oceanic Next",
            Self::RosePine => "Rose Pine",
            Self::Everforest => "Everforest",
            Self::Kanagawa => "Kanagawa",
            Self::AyuMirage => "Ayu Mirage",
            Self::NightOwl => "Night Owl",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::MonokaiBlue => "charcoal, tan text, warm blue accents",
            Self::TokyoNight => "deep navy with cool blue highlights",
            Self::CatppuccinMocha => "soft mocha with lavender and peach",
            Self::Gruvbox => "warm retro brown with gold accents",
            Self::Nord => "arctic blue-gray with frost accents",
            Self::Dracula => "dark purple with vivid pink and cyan",
            Self::Aura => "vibrant purple with neon green accents",
            Self::SolarizedDark => "classic low-contrast teal and yellow",
            Self::OceanicNext => "deep sea blue with coral and mint",
            Self::RosePine => "relaxed dusky pink and muted gold",
            Self::Everforest => "soft organic green and warm earth tones",
            Self::Kanagawa => "sophisticated old-world Japanese waves",
            Self::AyuMirage => "modern minimal charcoal and orange",
            Self::NightOwl => "accessible deep blue and vibrant neon",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|theme| *theme == self)
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningEffort {
    Auto,
    Low,
    Medium,
    High,
    XHigh,
}

impl ReasoningEffort {
    pub const AUTO_ONLY: [Self; 1] = [Self::Auto];
    pub const STANDARD: [Self; 4] = [Self::Auto, Self::Low, Self::Medium, Self::High];
    pub const EXTENDED: [Self; 5] = [Self::Auto, Self::Low, Self::Medium, Self::High, Self::XHigh];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
        }
    }

    fn request_value(self) -> Option<String> {
        match self {
            Self::Auto => None,
            _ => Some(self.label().to_owned()),
        }
    }

    fn next_in(self, supported: &[Self]) -> Self {
        if supported.is_empty() {
            return Self::Auto;
        }
        let index = supported
            .iter()
            .position(|effort| *effort == self)
            .unwrap_or(0);
        supported[(index + 1) % supported.len()]
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Quote {
    #[serde(rename = "q")]
    pub text: String,
    #[serde(rename = "a")]
    pub author: String,
}

#[derive(Debug)]
pub enum AppEvent {
    Model(ModelEvent),
    Auth(AuthEvent),
    Quote(Quote),
}

#[derive(Debug)]
pub enum AuthEvent {
    Status(String),
    Message(String),
    CopilotModels(Result<Vec<String>, String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputAction {
    Insert(char),
    Backspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashCommand {
    pub name: &'static str,
    pub description: &'static str,
}

pub const SLASH_COMMANDS: [SlashCommand; 10] = [
    SlashCommand {
        name: "/help",
        description: "Show available artui commands",
    },
    SlashCommand {
        name: "/theme",
        description: "Choose a color palette",
    },
    SlashCommand {
        name: "/model",
        description: "Switch the active model",
    },
    SlashCommand {
        name: "/statusline",
        description: "Configure statusline items",
    },
    SlashCommand {
        name: "/agent",
        description: "Switch between build and plan agents",
    },
    SlashCommand {
        name: "/login",
        description: "Connect an account provider",
    },
    SlashCommand {
        name: "/logout",
        description: "Remove saved provider credentials",
    },
    SlashCommand {
        name: "/clear",
        description: "Clear the current transcript",
    },
    SlashCommand {
        name: "/quit",
        description: "Quit artui",
    },
    SlashCommand {
        name: "/exit",
        description: "Quit artui",
    },
];

const FALLBACK_THINKING_PHRASE: &str = "Working";
const FALLBACK_SPINNER_FRAME: &str = "•";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLineItem {
    Model,
    Agent,
    Reasoning,
    ProviderUsage,
    CurrentDir,
    ProjectName,
    GitBranch,
    Context,
    GitStatus,
    EscHint,
}

impl StatusLineItem {
    pub const ALL: [Self; 10] = [
        Self::Model,
        Self::Agent,
        Self::Reasoning,
        Self::ProviderUsage,
        Self::CurrentDir,
        Self::ProjectName,
        Self::GitBranch,
        Self::Context,
        Self::GitStatus,
        Self::EscHint,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Agent => "agent",
            Self::Reasoning => "reasoning",
            Self::ProviderUsage => "usage",
            Self::CurrentDir => "current-dir",
            Self::ProjectName => "project-name",
            Self::GitBranch => "git-branch",
            Self::Context => "context",
            Self::GitStatus => "git-status",
            Self::EscHint => "esc-hint",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Model => "Current model name",
            Self::Agent => "Current agent profile",
            Self::Reasoning => "Provider reasoning effort",
            Self::ProviderUsage => "Current provider usage",
            Self::CurrentDir => "Current working directory",
            Self::ProjectName => "Project directory name",
            Self::GitBranch => "Current Git branch",
            Self::Context => "Context usage indicator",
            Self::GitStatus => "Git working tree summary",
            Self::EscHint => "Keyboard hint shown on the right",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|item| *item == self).unwrap_or(0)
    }
}

pub struct ProviderRequest {
    pub provider: Arc<dyn LlmProvider>,
    pub request: ModelRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOption {
    pub provider_id: String,
    pub provider_name: String,
    pub model: Option<String>,
    pub hint: Option<String>,
}

impl ModelOption {
    fn header(provider_id: &str, provider_name: &str) -> Self {
        Self {
            provider_id: provider_id.to_owned(),
            provider_name: provider_name.to_owned(),
            model: None,
            hint: None,
        }
    }

    fn model(provider_id: &str, provider_name: &str, model: String, hint: Option<String>) -> Self {
        Self {
            provider_id: provider_id.to_owned(),
            provider_name: provider_name.to_owned(),
            model: Some(model),
            hint,
        }
    }

    fn is_selectable(&self) -> bool {
        self.model.is_some()
    }
}

pub enum AppRequest {
    Provider(ProviderRequest),
    GitHubDeviceLogin {
        config: GitHubDeviceFlowConfig,
        copilot_config: Box<CopilotConfig>,
        store: AuthStore,
    },
    RefreshCopilotModels {
        config: Box<CopilotConfig>,
        store: AuthStore,
    },
    FetchQuote,
}

enum SlashCommandResult {
    NotCommand,
    Handled(Option<AppRequest>),
}

pub struct App {
    pub config: AppConfig,
    pub provider: Arc<dyn LlmProvider>,
    pub auth_store: Option<AuthStore>,
    pub tool_registry: crate::tools::registry::ToolRegistry,
    pub mode: UiMode,
    pub transcript: Vec<Message>,
    pub input: String,
    pub status: String,
    pub should_quit: bool,
    pub slash_cursor: usize,
    pub chat_scroll: u16,
    pub logo: &'static str,
    pub active_agent: PrimaryAgent,
    pub reasoning_effort: ReasoningEffort,
    pub theme: ThemeId,
    pub theme_picker_open: bool,
    pub theme_cursor: usize,
    pub model_picker_open: bool,
    pub model_cursor: usize,
    pub model_scroll: usize,
    pub model_options: Vec<ModelOption>,
    pub login_picker_open: bool,
    pub login_cursor: usize,
    pub agent_picker_open: bool,
    pub agent_cursor: usize,
    pub statusline_open: bool,
    pub statusline_cursor: usize,
    pub statusline_enabled: [bool; StatusLineItem::ALL.len()],
    pub git_branch_label: String,
    pub git_status_label: String,
    pub thinking_frame: usize,
    pub thinking_phrase: usize,
    pub thinking_started_at: Option<Instant>,
    pub last_thinking_frame_at: Instant,
    pub last_thinking_phrase_at: Instant,
    pub eye_frame: usize,
    pub last_eye_frame_at: Instant,
    pub quote: Option<Quote>,
}

impl App {
    pub fn new(config: AppConfig, provider: Arc<dyn LlmProvider>) -> Self {
        let auth_store = AuthStore::from_config(&config);
        let model_options = available_model_options(&config, auth_store.as_ref());
        let now = Instant::now();
        Self {
            status: format!("Provider: {}", config.default_provider),
            config,
            provider,
            auth_store,
            tool_registry: crate::tools::registry::ToolRegistry::new(),
            mode: UiMode::Input,
            transcript: Vec::new(),
            input: String::new(),
            should_quit: false,
            slash_cursor: 0,
            chat_scroll: 0,
            logo: LOGO,
            active_agent: PrimaryAgent::Build,
            reasoning_effort: ReasoningEffort::Auto,
            theme: ThemeId::MonokaiBlue,
            theme_picker_open: false,
            theme_cursor: ThemeId::MonokaiBlue.index(),
            model_picker_open: false,
            model_cursor: 0,
            model_scroll: 0,
            model_options,
            login_picker_open: false,
            login_cursor: 0,
            agent_picker_open: false,
            agent_cursor: PrimaryAgent::Build.index(),
            statusline_open: false,
            statusline_cursor: 0,
            statusline_enabled: [true; StatusLineItem::ALL.len()],
            git_branch_label: git_branch().unwrap_or_else(|| "no-git".to_owned()),
            git_status_label: git_status_label().unwrap_or_else(|| "unknown".to_owned()),
            thinking_frame: 0,
            thinking_phrase: 0,
            thinking_started_at: None,
            last_thinking_frame_at: now,
            last_thinking_phrase_at: now,
            eye_frame: 0,
            last_eye_frame_at: now,
            quote: None,
        }
    }

    pub fn active_agent_name(&self) -> &'static str {
        self.active_agent.name()
    }

    pub fn active_agent_id(&self) -> &'static str {
        self.active_agent.id()
    }

    pub fn active_agent_description(&self) -> &'static str {
        self.active_agent.description()
    }

    pub fn cycle_reasoning_effort(&mut self) {
        if self.mode == UiMode::Streaming {
            return;
        }
        self.reasoning_effort = self
            .reasoning_effort
            .next_in(&self.supported_reasoning_efforts());
        self.status = format!(
            "Reasoning effort: {} ({})",
            self.reasoning_effort.label(),
            self.config.default_provider
        );
    }

    pub fn supported_reasoning_efforts(&self) -> Vec<ReasoningEffort> {
        if self.config.default_provider == "copilot" {
            if let Some(efforts) =
                copilot_model_reasoning_efforts(self.auth_store.as_ref(), self.active_model())
            {
                return efforts;
            }
        }
        provider_reasoning_efforts(&self.config.default_provider, self.active_model())
    }

    fn normalized_reasoning_effort(&self) -> ReasoningEffort {
        if self
            .supported_reasoning_efforts()
            .contains(&self.reasoning_effort)
        {
            self.reasoning_effort
        } else {
            ReasoningEffort::Auto
        }
    }

    pub fn cycle_agent(&mut self) {
        if self.mode == UiMode::Streaming
            || self.theme_picker_open
            || self.model_picker_open
            || self.login_picker_open
            || self.statusline_open
            || self.agent_picker_open
            || !self.input.trim().is_empty()
        {
            return;
        }

        self.active_agent = if self.active_agent == PrimaryAgent::Build {
            PrimaryAgent::Plan
        } else {
            PrimaryAgent::Build
        };
        self.agent_cursor = self.active_agent.index();
        self.status = format!("Agent: {}", self.active_agent.id());
    }

    pub fn open_agent_picker(&mut self) {
        self.theme_picker_open = false;
        self.model_picker_open = false;
        self.login_picker_open = false;
        self.statusline_open = false;
        self.agent_picker_open = true;
        self.agent_cursor = self.active_agent.index();
        self.mode = UiMode::Normal;
        self.status = "Select an agent".to_owned();
    }

    pub fn next_agent(&mut self) {
        if self.agent_picker_open {
            self.agent_cursor = (self.agent_cursor + 1) % PrimaryAgent::ALL.len();
        }
    }

    pub fn previous_agent(&mut self) {
        if self.agent_picker_open {
            self.agent_cursor =
                (self.agent_cursor + PrimaryAgent::ALL.len() - 1) % PrimaryAgent::ALL.len();
        }
    }

    pub fn select_agent(&mut self) {
        if !self.agent_picker_open {
            return;
        }
        self.active_agent = PrimaryAgent::ALL[self.agent_cursor];
        self.agent_picker_open = false;
        self.mode = UiMode::Input;
        self.status = format!("Agent: {}", self.active_agent.id());
    }

    fn close_agent_picker(&mut self) {
        self.agent_picker_open = false;
        self.mode = UiMode::Input;
        self.status = self.provider_status();
    }

    fn provider_status(&self) -> String {
        format!(
            "Provider: {} • Agent: {} • Reasoning: {}",
            self.config.default_provider,
            self.active_agent.id(),
            self.reasoning_effort.label()
        )
    }

    pub fn edit_input(&mut self, action: InputAction) {
        if self.mode == UiMode::Streaming
            || self.theme_picker_open
            || self.model_picker_open
            || self.login_picker_open
            || self.statusline_open
            || self.agent_picker_open
        {
            return;
        }

        self.mode = UiMode::Input;
        match action {
            InputAction::Insert(ch) => {
                self.input.push(ch);
                self.clamp_slash_cursor();
            }
            InputAction::Backspace => {
                self.input.pop();
                self.clamp_slash_cursor();
            }
        }
    }

    pub fn submit_input(&mut self) -> Option<AppRequest> {
        if self.mode == UiMode::Streaming {
            return None;
        }

        let content = self.input.trim().to_owned();
        if content.is_empty() {
            return None;
        }
        if let SlashCommandResult::Handled(request) = self.run_slash_command(&content) {
            self.input.clear();
            self.slash_cursor = 0;
            return request;
        }

        self.input.clear();
        self.slash_cursor = 0;
        self.chat_scroll = 0;
        self.transcript.push(Message {
            role: Role::User,
            content: content.clone(),
        });
        self.transcript.push(Message {
            role: Role::Assistant,
            content: String::new(),
        });
        self.mode = UiMode::Streaming;
        self.status = "Streaming response".to_owned();
        self.start_thinking_animation();

        Some(AppRequest::Provider(ProviderRequest {
            provider: Arc::clone(&self.provider),
            request: ModelRequest {
                messages: self.transcript.clone(),
                system_prompt: Some(self.system_prompt()),
                reasoning_effort: self.normalized_reasoning_effort().request_value(),
                tools: self.tool_registry.specs(),
                tool_choice: crate::providers::ToolChoice::Auto,
                max_output_tokens: None,
            },
        }))
    }

    fn system_prompt(&self) -> String {
        format!(
            "You are artui, an interactive coding-agent CLI. You are not ChatGPT in this product UI. If asked who you are, identify as artui and state the active provider/model exactly as {}/{}. Do not claim you lack a model label.\n\n{}",
            self.config.default_provider,
            self.active_model(),
            self.active_agent.system_prompt()
        )
    }

    pub fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Model(ModelEvent::TextDelta(token)) => self.append_assistant_token(&token),
            AppEvent::Model(ModelEvent::Done { .. }) => {
                self.mode = UiMode::Input;
                self.status = self.provider_status();
                self.stop_thinking_animation();
            }
            AppEvent::Auth(AuthEvent::Status(status)) => {
                self.status = status;
                self.mode = UiMode::Input;
            }
            AppEvent::Auth(AuthEvent::Message(message)) => {
                self.transcript.push(Message {
                    role: Role::Assistant,
                    content: message,
                });
                self.mode = UiMode::Input;
            }
            AppEvent::Auth(AuthEvent::CopilotModels(result)) => match result {
                Ok(models) => {
                    if self.config.default_provider == "copilot"
                        && !models.iter().any(|model| model == self.active_model())
                    {
                        if let Some(model) = models.first() {
                            self.config.providers.copilot.default_model = model.clone();
                            if let Ok(provider) = build_provider(&self.config) {
                                self.provider = provider;
                            }
                        }
                    }
                    self.config.providers.copilot.models = models;
                    if self.model_picker_open {
                        self.model_options =
                            available_model_options(&self.config, self.auth_store.as_ref());
                        self.model_cursor = self
                            .model_options
                            .iter()
                            .position(|option| {
                                option.provider_id == "copilot" && option.is_selectable()
                            })
                            .or_else(|| first_selectable_model_index(&self.model_options))
                            .unwrap_or(0);
                        self.ensure_model_cursor_visible();
                    }
                    self.status = format!(
                        "GitHub Copilot models refreshed: {}",
                        self.config.providers.copilot.models.len()
                    );
                    self.mode = if self.model_picker_open {
                        UiMode::Normal
                    } else {
                        UiMode::Input
                    };
                }
                Err(error) => {
                    let message = format!("Copilot model refresh failed: {error}");
                    self.status = message.clone();
                    self.push_login_message(message);
                    self.mode = if self.model_picker_open {
                        UiMode::Normal
                    } else {
                        UiMode::Input
                    };
                }
            },
            AppEvent::Model(ModelEvent::Error(error)) => {
                self.append_assistant_token(&format!("\nError: {error}"));
                self.mode = UiMode::Input;
                self.status = "Provider error".to_owned();
                self.stop_thinking_animation();
            }
            // Tool-call and metadata events — no-op until agent loop (Phase C)
            AppEvent::Model(ModelEvent::ToolCallStart { .. })
            | AppEvent::Model(ModelEvent::ToolCallArgsDelta { .. })
            | AppEvent::Model(ModelEvent::ToolCallEnd { .. })
            | AppEvent::Model(ModelEvent::ReasoningDelta(_))
            | AppEvent::Model(ModelEvent::Usage { .. }) => {}
            AppEvent::Quote(quote) => {
                self.quote = Some(quote);
            }
        }
    }

    pub fn cancel_input(&mut self) {
        if self.theme_picker_open {
            self.close_theme_picker();
            return;
        }
        if self.model_picker_open {
            self.close_model_picker();
            return;
        }
        if self.login_picker_open {
            self.close_login_picker();
            return;
        }
        if self.statusline_open {
            self.close_statusline_picker();
            return;
        }
        if self.agent_picker_open {
            self.close_agent_picker();
            return;
        }
        if self.mode != UiMode::Streaming {
            self.input.clear();
            self.slash_cursor = 0;
            self.mode = UiMode::Normal;
        }
    }

    pub fn open_theme_picker(&mut self) {
        self.statusline_open = false;
        self.model_picker_open = false;
        self.login_picker_open = false;
        self.agent_picker_open = false;
        self.theme_picker_open = true;
        self.theme_cursor = self.theme.index();
        self.mode = UiMode::Normal;
        self.status = "Select a theme".to_owned();
    }

    pub fn next_theme(&mut self) {
        if self.theme_picker_open {
            self.theme_cursor = (self.theme_cursor + 1) % ThemeId::ALL.len();
        }
    }

    pub fn previous_theme(&mut self) {
        if self.theme_picker_open {
            self.theme_cursor = (self.theme_cursor + ThemeId::ALL.len() - 1) % ThemeId::ALL.len();
        }
    }

    pub fn select_theme(&mut self) {
        if self.theme_picker_open {
            self.theme = ThemeId::ALL[self.theme_cursor];
            self.theme_picker_open = false;
            self.mode = UiMode::Input;
            self.status = format!("Theme: {}", self.theme.name());
        }
    }

    fn close_theme_picker(&mut self) {
        self.theme_picker_open = false;
        self.mode = UiMode::Input;
        self.status = self.provider_status();
    }

    pub fn open_statusline_picker(&mut self) {
        self.theme_picker_open = false;
        self.model_picker_open = false;
        self.login_picker_open = false;
        self.agent_picker_open = false;
        self.statusline_open = true;
        self.statusline_cursor = self
            .statusline_cursor
            .min(StatusLineItem::ALL.len().saturating_sub(1));
        self.mode = UiMode::Normal;
        self.status = "Configure statusline".to_owned();
    }

    pub fn next_statusline_item(&mut self) {
        if self.statusline_open {
            self.statusline_cursor = (self.statusline_cursor + 1) % StatusLineItem::ALL.len();
        }
    }

    pub fn previous_statusline_item(&mut self) {
        if self.statusline_open {
            self.statusline_cursor = (self.statusline_cursor + StatusLineItem::ALL.len() - 1)
                % StatusLineItem::ALL.len();
        }
    }

    pub fn toggle_statusline_item(&mut self) {
        if self.statusline_open {
            let item = StatusLineItem::ALL[self.statusline_cursor];
            let index = item.index();
            self.statusline_enabled[index] = !self.statusline_enabled[index];
        }
    }

    pub fn select_statusline(&mut self) {
        if self.statusline_open {
            self.close_statusline_picker();
            self.status = "Statusline updated".to_owned();
        }
    }

    fn close_statusline_picker(&mut self) {
        self.statusline_open = false;
        self.mode = UiMode::Input;
        self.status = self.provider_status();
    }

    pub fn open_model_picker(&mut self) -> Option<AppRequest> {
        self.theme_picker_open = false;
        self.statusline_open = false;
        self.login_picker_open = false;
        self.agent_picker_open = false;
        self.model_options = available_model_options(&self.config, self.auth_store.as_ref());
        let active_model = self.active_model().to_owned();
        self.model_cursor = self
            .model_options
            .iter()
            .position(|option| {
                option.provider_id == self.config.default_provider
                    && option.model.as_deref() == Some(active_model.as_str())
            })
            .or_else(|| first_selectable_model_index(&self.model_options))
            .unwrap_or(0);
        self.model_scroll = 0;
        self.model_picker_open = true;
        self.mode = UiMode::Normal;
        self.status = "Select a model".to_owned();
        self.copilot_model_refresh_request()
    }

    pub fn next_model(&mut self) {
        if self.model_picker_open && !self.model_options.is_empty() {
            self.model_cursor = next_selectable_model_index(&self.model_options, self.model_cursor)
                .unwrap_or(self.model_cursor);
            self.ensure_model_cursor_visible();
        }
    }

    pub fn previous_model(&mut self) {
        if self.model_picker_open && !self.model_options.is_empty() {
            self.model_cursor =
                previous_selectable_model_index(&self.model_options, self.model_cursor)
                    .unwrap_or(self.model_cursor);
            self.ensure_model_cursor_visible();
        }
    }

    pub fn select_model(&mut self) {
        if !self.model_picker_open {
            return;
        }
        if let Some(option) = self.model_options.get(self.model_cursor).cloned() {
            if let Some(model) = option.model {
                self.switch_active_model(option.provider_id, model);
            }
        }
        self.model_picker_open = false;
        self.mode = UiMode::Input;
    }

    fn close_model_picker(&mut self) {
        self.model_picker_open = false;
        self.model_scroll = 0;
        self.mode = UiMode::Input;
        self.status = format!("Model: {}", self.active_model());
    }

    fn ensure_model_cursor_visible(&mut self) {
        const MODEL_PICKER_VISIBLE_ROWS: usize = 12;
        if self.model_cursor < self.model_scroll {
            self.model_scroll = self.model_cursor;
        } else if self.model_cursor >= self.model_scroll.saturating_add(MODEL_PICKER_VISIBLE_ROWS) {
            self.model_scroll = self
                .model_cursor
                .saturating_add(1)
                .saturating_sub(MODEL_PICKER_VISIBLE_ROWS);
        }
        self.model_scroll = self.model_scroll.min(
            self.model_options
                .len()
                .saturating_sub(MODEL_PICKER_VISIBLE_ROWS),
        );
    }

    pub fn open_login_picker(&mut self) {
        self.theme_picker_open = false;
        self.model_picker_open = false;
        self.statusline_open = false;
        self.agent_picker_open = false;
        self.login_picker_open = true;
        self.login_cursor = self.login_cursor.min(PROVIDERS.len().saturating_sub(1));
        self.mode = UiMode::Normal;
        self.status = "Select a provider to log in".to_owned();
    }

    pub fn next_login_provider(&mut self) {
        if self.login_picker_open {
            self.login_cursor = (self.login_cursor + 1) % PROVIDERS.len();
        }
    }

    pub fn previous_login_provider(&mut self) {
        if self.login_picker_open {
            self.login_cursor = (self.login_cursor + PROVIDERS.len() - 1) % PROVIDERS.len();
        }
    }

    pub fn select_login_provider(&mut self) -> Option<AppRequest> {
        if !self.login_picker_open {
            return None;
        }
        let provider_id = PROVIDERS[self.login_cursor].id;
        self.login_picker_open = false;
        self.mode = UiMode::Input;
        self.push_login_message(format!(
            "Selected {}.",
            PROVIDERS[self.login_cursor].display_name
        ));
        self.login_provider(provider_id)
    }

    fn close_login_picker(&mut self) {
        self.login_picker_open = false;
        self.mode = UiMode::Input;
        self.status = self.provider_status();
    }

    pub fn active_model(&self) -> &str {
        active_model_from_config(&self.config)
    }

    fn switch_active_model(&mut self, provider_id: String, model: String) {
        let mut next_config = self.config.clone();
        match provider_id.as_str() {
            "ollama" => next_config.providers.ollama.default_model = model.clone(),
            "openai_compat" => next_config.providers.openai_compat.default_model = model.clone(),
            "copilot" => next_config.providers.copilot.default_model = model.clone(),
            "openai_account" => next_config.providers.openai_account.default_model = model.clone(),
            provider => {
                self.status = format!("Cannot switch model for unsupported provider: {provider}");
                return;
            }
        }
        next_config.default_provider = provider_id.clone();

        match build_provider(&next_config) {
            Ok(provider) => {
                self.config = next_config;
                self.provider = provider;
                if !self
                    .supported_reasoning_efforts()
                    .contains(&self.reasoning_effort)
                {
                    self.reasoning_effort = ReasoningEffort::Auto;
                }
                self.status = format!("Model: {provider_id}/{model}");
            }
            Err(error) => {
                self.status = format!("Model switch failed: {error}");
            }
        }
    }

    pub fn clear_transcript(&mut self) {
        self.transcript.clear();
        self.chat_scroll = 0;
        self.status = "Transcript cleared".to_owned();
    }

    pub fn advance_thinking_animation(&mut self) {
        let now = Instant::now();

        // Eye animation (always active, context-aware)
        let interval = match self.mode {
            UiMode::Streaming => Duration::from_millis(250),
            UiMode::Input if !self.input.is_empty() => Duration::from_millis(200),
            _ => Duration::from_millis(400),
        };

        if now.duration_since(self.last_eye_frame_at) >= interval {
            let frames = self.active_eye_frames();
            self.eye_frame = next_wrapped_index(self.eye_frame, frames.len());
            self.last_eye_frame_at = now;
        }

        if self.mode != UiMode::Streaming {
            return;
        }

        if now.duration_since(self.last_thinking_frame_at) >= self.spinner_interval() {
            self.thinking_frame =
                next_wrapped_index(self.thinking_frame, self.spinner_frame_count());
            self.last_thinking_frame_at = now;
        }

        if now.duration_since(self.last_thinking_phrase_at) >= self.phrase_interval() {
            self.thinking_phrase =
                next_wrapped_index(self.thinking_phrase, self.thinking_phrase_count());
            self.last_thinking_phrase_at = now;
        }
    }

    pub fn eye_frame(&self) -> &str {
        let frames = self.active_eye_frames();
        frames[self.eye_frame.min(frames.len() - 1)]
    }

    fn active_eye_frames(&self) -> &'static [&'static str] {
        match self.mode {
            UiMode::Streaming => {
                if self
                    .transcript
                    .last()
                    .map(|m| m.content.is_empty())
                    .unwrap_or(true)
                {
                    EYE_THINKING
                } else {
                    EYE_STREAMING
                }
            }
            UiMode::Input if !self.input.is_empty() => EYE_TYPING,
            _ => EYE_IDLE,
        }
    }

    pub fn thinking_frame(&self) -> &str {
        self.config
            .ui
            .spinner_frames
            .get(self.thinking_frame)
            .map(String::as_str)
            .filter(|frame| !frame.is_empty())
            .unwrap_or(FALLBACK_SPINNER_FRAME)
    }

    pub fn thinking_phrase(&self) -> &str {
        if self.active_model_has_reasoning() {
            return self
                .config
                .ui
                .reasoning_phrases
                .get(self.thinking_phrase)
                .map(String::as_str)
                .filter(|phrase| !phrase.is_empty())
                .unwrap_or(FALLBACK_THINKING_PHRASE);
        }

        self.config
            .ui
            .thinking_phrases
            .get(self.thinking_phrase)
            .map(String::as_str)
            .filter(|phrase| !phrase.is_empty())
            .unwrap_or(FALLBACK_THINKING_PHRASE)
    }

    pub fn thinking_elapsed(&self) -> Option<Duration> {
        self.thinking_started_at.map(|started| started.elapsed())
    }

    pub fn scroll_chat_up(&mut self) {
        if self.can_scroll_chat() {
            self.chat_scroll = self.chat_scroll.saturating_add(CHAT_SCROLL_STEP);
        }
    }

    pub fn scroll_chat_down(&mut self) {
        self.chat_scroll = self.chat_scroll.saturating_sub(CHAT_SCROLL_STEP);
    }

    pub fn page_chat_up(&mut self) {
        if self.can_scroll_chat() {
            self.chat_scroll = self.chat_scroll.saturating_add(CHAT_PAGE_SCROLL_STEP);
        }
    }

    pub fn page_chat_down(&mut self) {
        self.chat_scroll = self.chat_scroll.saturating_sub(CHAT_PAGE_SCROLL_STEP);
    }

    fn can_scroll_chat(&self) -> bool {
        !self.transcript.is_empty()
            && !self.theme_picker_open
            && !self.model_picker_open
            && !self.login_picker_open
            && !self.statusline_open
            && !self.agent_picker_open
            && !self.has_slash_command_matches()
    }

    pub fn complete_slash_command(&mut self) {
        if let Some(command) = self.selected_slash_command() {
            self.input = command.name.to_owned();
            self.mode = UiMode::Input;
        }
    }

    pub fn submit_slash_command_selection(&mut self) -> Option<AppRequest> {
        if let Some(command) = self.selected_slash_command() {
            self.input = command.name.to_owned();
            return self.submit_input();
        }
        None
    }

    pub fn has_slash_command_matches(&self) -> bool {
        !slash_command_matches(self.input.as_str()).is_empty()
    }

    pub fn next_slash_command(&mut self) {
        let matches = slash_command_matches(self.input.as_str());
        if !matches.is_empty() {
            self.slash_cursor = (self.slash_cursor + 1) % matches.len();
        }
    }

    pub fn previous_slash_command(&mut self) {
        let matches = slash_command_matches(self.input.as_str());
        if !matches.is_empty() {
            self.slash_cursor = (self.slash_cursor + matches.len() - 1) % matches.len();
        }
    }

    fn selected_slash_command(&self) -> Option<&'static SlashCommand> {
        let matches = slash_command_matches(self.input.as_str());
        matches
            .get(self.slash_cursor.min(matches.len().saturating_sub(1)))
            .copied()
    }

    fn clamp_slash_cursor(&mut self) {
        let matches = slash_command_matches(self.input.as_str());
        if matches.is_empty() {
            self.slash_cursor = 0;
        } else {
            self.slash_cursor = self.slash_cursor.min(matches.len() - 1);
        }
    }

    fn run_slash_command(&mut self, content: &str) -> SlashCommandResult {
        match content {
            "/help" => {
                self.show_help();
                SlashCommandResult::Handled(None)
            }
            "/theme" => {
                self.open_theme_picker();
                SlashCommandResult::Handled(None)
            }
            "/model" => SlashCommandResult::Handled(self.open_model_picker()),
            "/model refresh" => {
                self.status = "Refreshing GitHub Copilot models".to_owned();
                SlashCommandResult::Handled(self.copilot_model_refresh_request())
            }
            "/statusline" => {
                self.open_statusline_picker();
                SlashCommandResult::Handled(None)
            }
            "/agent" => {
                self.open_agent_picker();
                SlashCommandResult::Handled(None)
            }
            "/clear" => {
                self.clear_transcript();
                self.mode = UiMode::Input;
                SlashCommandResult::Handled(None)
            }
            "/quit" | "/exit" => {
                self.should_quit = true;
                SlashCommandResult::Handled(None)
            }
            command if command.starts_with("/model ") => {
                let model = command.trim_start_matches("/model").trim();
                if model.is_empty() {
                    return SlashCommandResult::Handled(self.open_model_picker());
                } else if model == "refresh" {
                    self.status = "Refreshing GitHub Copilot models".to_owned();
                    return SlashCommandResult::Handled(self.copilot_model_refresh_request());
                } else {
                    self.switch_active_model(
                        self.config.default_provider.clone(),
                        model.to_owned(),
                    );
                }
                SlashCommandResult::Handled(None)
            }
            command if command.starts_with("/agent ") => {
                let requested = command.trim_start_matches("/agent").trim();
                match PrimaryAgent::from_id(requested) {
                    Some(agent) => {
                        self.active_agent = agent;
                        self.agent_cursor = agent.index();
                        self.status = format!("Agent: {}", agent.id());
                    }
                    None => {
                        self.status = format!("Unknown agent: {requested}");
                    }
                }
                SlashCommandResult::Handled(None)
            }
            command if command.starts_with("/login ") => {
                let request = self.login_provider(command.trim_start_matches("/login").trim());
                SlashCommandResult::Handled(request)
            }
            command if command.starts_with("/logout ") => {
                self.logout_provider(command.trim_start_matches("/logout").trim());
                SlashCommandResult::Handled(None)
            }
            "/login" => {
                self.open_login_picker();
                SlashCommandResult::Handled(None)
            }
            "/logout" => {
                self.status = "Usage: /logout <provider>".to_owned();
                SlashCommandResult::Handled(None)
            }
            command if command.starts_with('/') => {
                self.status = format!("Unknown command: {command}");
                SlashCommandResult::Handled(None)
            }
            _ => SlashCommandResult::NotCommand,
        }
    }

    fn show_help(&mut self) {
        let commands = SLASH_COMMANDS
            .iter()
            .map(|command| format!("{} — {}", command.name, command.description))
            .collect::<Vec<_>>()
            .join("\n");
        self.transcript.push(Message {
            role: Role::Assistant,
            content: format!("Available commands:\n{commands}"),
        });
        self.mode = UiMode::Input;
        self.status = "Showing commands".to_owned();
    }

    #[allow(dead_code)]
    fn show_providers(&mut self) {
        let lines = PROVIDERS
            .iter()
            .map(|provider| {
                format!(
                    "{} — {} — auth: {} — models: {} — streaming: {}",
                    provider.id,
                    self.provider_status_label(provider.id),
                    provider.auth_requirement.label(),
                    provider.model_list_strategy.label(),
                    if provider.streaming { "yes" } else { "not yet" }
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let auth_path = self
            .auth_store
            .as_ref()
            .map(|store| store.path().display().to_string())
            .unwrap_or_else(|| "unavailable".to_owned());
        self.transcript.push(Message {
            role: Role::Assistant,
            content: format!(
                "Providers:\n{lines}\n\nAuth store: {auth_path}\nUse /login <provider> or /logout <provider> for account-backed providers."
            ),
        });
        self.mode = UiMode::Input;
        self.status = "Showing providers".to_owned();
    }

    fn login_provider(&mut self, provider_id: &str) -> Option<AppRequest> {
        let provider_id = provider_id.trim().to_ascii_lowercase();
        if provider_id == "copilot --env" {
            self.login_copilot_from_env();
            return None;
        }
        let Some(metadata) = registry::provider_metadata(&provider_id) else {
            self.status = format!("Unknown provider: {provider_id}");
            return None;
        };

        match metadata.auth_requirement {
            AuthRequirement::None => {
                self.status = format!("{} does not require login", metadata.display_name);
                self.push_login_message(self.status.clone());
                None
            }
            AuthRequirement::ApiKey => {
                self.status = format!(
                    "{} uses its configured API key environment variable",
                    metadata.display_name
                );
                self.push_login_message(self.status.clone());
                None
            }
            AuthRequirement::Account if provider_id == "copilot" => {
                self.login_copilot_device_flow()
            }
            AuthRequirement::Account => {
                self.status = format!(
                    "{} login is not implemented until an official third-party OAuth flow is configured",
                    metadata.display_name
                );
                self.push_login_message(self.status.clone());
                None
            }
        }
    }

    fn login_copilot_device_flow(&mut self) -> Option<AppRequest> {
        let Some(store) = self.auth_store.clone() else {
            self.status = "Auth store is unavailable on this platform".to_owned();
            return None;
        };

        let config = match copilot_device_flow_config(&self.config.providers.copilot) {
            Ok(config) => config,
            Err(message) => {
                self.status = message;
                self.push_login_message(self.status.clone());
                return None;
            }
        };

        self.status = "Starting GitHub device login".to_owned();
        self.push_login_message(
            "Starting GitHub Copilot device login. A browser tab should open after GitHub returns a device code.".to_owned(),
        );
        Some(AppRequest::GitHubDeviceLogin {
            config,
            copilot_config: Box::new(self.config.providers.copilot.clone()),
            store,
        })
    }

    fn login_copilot_from_env(&mut self) {
        let Some(store) = &self.auth_store else {
            self.status = "Auth store is unavailable on this platform".to_owned();
            return;
        };

        let found = self
            .config
            .providers
            .copilot
            .github_token_env
            .iter()
            .find_map(|name| {
                std::env::var(name)
                    .ok()
                    .filter(|token| !token.is_empty())
                    .map(|token| (name, token))
            });
        let Some((env_name, token)) = found else {
            self.status =
                "Set a configured GitHub token env var, then run /login copilot --env".to_owned();
            self.push_login_message(self.status.clone());
            return;
        };

        let mut metadata = std::collections::BTreeMap::new();
        metadata.insert("source".to_owned(), "environment".to_owned());
        metadata.insert("env".to_owned(), env_name.clone());
        let record = AuthRecord {
            provider_id: "copilot".to_owned(),
            account_label: Some(format!("env:{env_name}")),
            access_token: Some(token),
            refresh_token: None,
            expires_at: None,
            updated_at: 0,
            metadata,
        };

        match store.upsert(record) {
            Ok(()) => self.status = "Copilot token saved from environment".to_owned(),
            Err(error) => self.status = format!("Login failed: {error}"),
        }
        self.push_login_message(self.status.clone());
    }

    fn push_login_message(&mut self, content: String) {
        self.transcript.push(Message {
            role: Role::Assistant,
            content,
        });
        self.chat_scroll = 0;
    }

    fn copilot_model_refresh_request(&self) -> Option<AppRequest> {
        let store = self.auth_store.clone()?;
        let connected = store
            .status("copilot")
            .ok()
            .is_some_and(|status| status == AuthStatus::Connected);
        connected.then(|| AppRequest::RefreshCopilotModels {
            config: Box::new(self.config.providers.copilot.clone()),
            store,
        })
    }

    fn logout_provider(&mut self, provider_id: &str) {
        let provider_id = provider_id.trim().to_ascii_lowercase();
        if registry::provider_metadata(&provider_id).is_none() {
            self.status = format!("Unknown provider: {provider_id}");
            return;
        }
        let Some(store) = &self.auth_store else {
            self.status = "Auth store is unavailable on this platform".to_owned();
            return;
        };

        match store.remove(&provider_id) {
            Ok(true) => {
                self.status = format!("Logged out of {provider_id}");
                // Clear cached models so they disappear from the picker
                if provider_id == "copilot" {
                    self.config.providers.copilot.models.clear();
                    self.model_options =
                        available_model_options(&self.config, self.auth_store.as_ref());
                }
            }
            Ok(false) => self.status = format!("{provider_id} was not connected"),
            Err(error) => self.status = format!("Logout failed: {error}"),
        }
    }

    /// Login the provider under the current model picker cursor.
    pub fn login_current_model_provider(&mut self) -> Option<AppRequest> {
        let provider_id = self
            .model_options
            .get(self.model_cursor)
            .map(|o| o.provider_id.clone())?;
        self.model_picker_open = false;
        self.mode = UiMode::Input;
        self.login_provider(&provider_id)
    }

    /// Logout the provider under the current model picker cursor.
    pub fn logout_current_model_provider(&mut self) {
        let Some(provider_id) = self
            .model_options
            .get(self.model_cursor)
            .map(|o| o.provider_id.clone())
        else {
            return;
        };
        self.logout_provider(&provider_id);
        // Refresh model list in picker
        self.model_options = available_model_options(&self.config, self.auth_store.as_ref());
        self.model_cursor = self
            .model_cursor
            .min(self.model_options.len().saturating_sub(1));
    }

    pub fn context_usage_label(&self) -> String {
        let used = self.estimated_context_tokens();
        let Some(limit) = self.active_context_window_tokens() else {
            return "ctx ?".to_owned();
        };
        let percent = used.saturating_mul(100) / limit.max(1);
        format!("ctx {}%", percent.min(100))
    }

    fn estimated_context_tokens(&self) -> usize {
        let chars = self.system_prompt().chars().count()
            + self.input.chars().count()
            + self
                .transcript
                .iter()
                .map(|message| message.content.chars().count() + 8)
                .sum::<usize>();
        chars.div_ceil(4).max(1)
    }

    fn active_context_window_tokens(&self) -> Option<usize> {
        if self.config.default_provider == "copilot" {
            if let Some(tokens) =
                copilot_model_context_window(self.auth_store.as_ref(), self.active_model())
            {
                return Some(tokens);
            }
        }
        known_context_window_tokens(&self.config.default_provider, self.active_model())
    }

    pub fn provider_usage_label(&self) -> String {
        registry::provider_metadata(&self.config.default_provider)
            .map(|provider| provider.display_name.to_owned())
            .unwrap_or_else(|| self.config.default_provider.clone())
    }

    pub fn provider_status_label(&self, provider_id: &str) -> String {
        match registry::provider_metadata(provider_id).map(|provider| provider.auth_requirement) {
            Some(AuthRequirement::None) => "available".to_owned(),
            Some(AuthRequirement::ApiKey) => {
                let env_name = self.config.providers.openai_compat.api_key_env.as_str();
                if std::env::var(env_name)
                    .ok()
                    .filter(|value| !value.is_empty())
                    .is_some()
                {
                    format!("configured via {env_name}")
                } else {
                    format!("missing env {env_name}")
                }
            }
            Some(AuthRequirement::Account) => {
                let Some(store) = &self.auth_store else {
                    return "not connected".to_owned();
                };
                let record = store.record(provider_id).ok().flatten();
                match record {
                    Some(rec) => {
                        let status = rec.status();
                        let source = rec
                            .metadata
                            .get("source")
                            .map(|s| format!(" via {s}"))
                            .unwrap_or_default();
                        format!("{}{source}", status.label())
                    }
                    None => "not connected".to_owned(),
                }
            }
            None => "unknown".to_owned(),
        }
    }

    fn append_assistant_token(&mut self, token: &str) {
        if let Some(message) = self
            .transcript
            .iter_mut()
            .rev()
            .find(|message| message.role == Role::Assistant)
        {
            message.content.push_str(token);
        }
    }

    fn start_thinking_animation(&mut self) {
        let now = Instant::now();
        self.thinking_frame = 0;
        self.thinking_phrase = self.random_thinking_phrase_index();
        self.thinking_started_at = Some(now);
        self.last_thinking_frame_at = now;
        self.last_thinking_phrase_at = now;
    }

    fn stop_thinking_animation(&mut self) {
        self.thinking_started_at = None;
    }

    fn random_thinking_phrase_index(&self) -> usize {
        let count = self.thinking_phrase_count();
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as usize)
            .unwrap_or(0);
        (seed ^ self.transcript.len() ^ self.active_model().len()) % count
    }

    fn spinner_interval(&self) -> Duration {
        Duration::from_millis(self.config.ui.spinner_interval_ms.max(1))
    }

    fn phrase_interval(&self) -> Duration {
        Duration::from_millis(self.config.ui.phrase_interval_ms.max(1))
    }

    fn spinner_frame_count(&self) -> usize {
        self.config
            .ui
            .spinner_frames
            .iter()
            .filter(|frame| !frame.is_empty())
            .count()
            .max(1)
    }

    fn thinking_phrase_count(&self) -> usize {
        let phrases = if self.active_model_has_reasoning() {
            &self.config.ui.reasoning_phrases
        } else {
            &self.config.ui.thinking_phrases
        };

        phrases
            .iter()
            .filter(|phrase| !phrase.is_empty())
            .count()
            .max(1)
    }

    fn active_model_has_reasoning(&self) -> bool {
        let active_model = self.active_model().to_ascii_lowercase();
        self.config
            .ui
            .reasoning_model_patterns
            .iter()
            .map(|pattern| pattern.trim().to_ascii_lowercase())
            .filter(|pattern| !pattern.is_empty())
            .any(|pattern| active_model.contains(&pattern))
    }
}

fn next_wrapped_index(index: usize, len: usize) -> usize {
    if len <= 1 {
        0
    } else {
        (index + 1) % len
    }
}

pub fn slash_command_matches(input: &str) -> Vec<&'static SlashCommand> {
    if !input.starts_with('/') || input.contains(char::is_whitespace) {
        return Vec::new();
    }

    SLASH_COMMANDS
        .iter()
        .filter(|command| command.name.starts_with(input))
        .collect()
}

impl From<ModelEvent> for AppEvent {
    fn from(value: ModelEvent) -> Self {
        Self::Model(value)
    }
}

fn git_branch() -> Option<String> {
    run_git(["branch", "--show-current"]).and_then(|branch| {
        if branch.is_empty() {
            run_git(["rev-parse", "--short", "HEAD"])
        } else {
            Some(branch)
        }
    })
}

fn git_status_label() -> Option<String> {
    run_git(["status", "--porcelain"]).map(|status| {
        if status.is_empty() {
            "clean".to_owned()
        } else {
            format!("{}", status.lines().count())
        }
    })
}

fn run_git<const N: usize>(args: [&str; N]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn known_context_window_tokens(provider_id: &str, model: &str) -> Option<usize> {
    let model = model.to_ascii_lowercase();
    let tokens = match provider_id {
        "ollama" => {
            if model.contains("nemotron") || model.contains("qwen3") {
                128_000
            } else {
                32_000
            }
        }
        "copilot" | "openai_account" | "openai_compat" => {
            if model.contains("gpt-5") || model.contains("codex") {
                400_000
            } else if model.contains("gpt-4.1") {
                1_000_000
            } else if model.contains("gpt-4o")
                || model.starts_with("o1")
                || model.starts_with("o3")
                || model.starts_with("o4")
            {
                128_000
            } else if model.contains("claude") || model.contains("gemini") {
                200_000
            } else {
                128_000
            }
        }
        _ => return None,
    };
    Some(tokens)
}

fn provider_reasoning_efforts(provider_id: &str, model: &str) -> Vec<ReasoningEffort> {
    let model = model.to_ascii_lowercase();
    match provider_id {
        // Ollama's native chat API does not expose a portable effort enum in artui yet.
        "ollama" => ReasoningEffort::AUTO_ONLY.to_vec(),
        // GitHub Copilot fallback when endpoint metadata is not cached yet.
        "copilot" if model.contains("gpt-5.5") => ReasoningEffort::EXTENDED.to_vec(),
        "copilot" if is_reasoning_model_name(&model) => ReasoningEffort::STANDARD.to_vec(),
        "copilot" => ReasoningEffort::AUTO_ONLY.to_vec(),
        // Account/API-backed OpenAI-compatible providers are model-dependent. Allow xhigh for
        // model families documented or commonly gateway-normalized with extended effort support.
        "openai_account" | "openai_compat" if supports_xhigh_reasoning(&model) => {
            ReasoningEffort::EXTENDED.to_vec()
        }
        "openai_account" | "openai_compat" if is_reasoning_model_name(&model) => {
            ReasoningEffort::STANDARD.to_vec()
        }
        "openai_account" | "openai_compat" => ReasoningEffort::AUTO_ONLY.to_vec(),
        _ => ReasoningEffort::AUTO_ONLY.to_vec(),
    }
}

fn supports_xhigh_reasoning(model: &str) -> bool {
    model.contains("gpt-5.5") || model.contains("xhigh")
}

fn is_reasoning_model_name(model: &str) -> bool {
    model.contains("codex")
        || model.starts_with("gpt-5")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
        || model.contains("reason")
        || model.contains("thinking")
}

fn active_model_from_config(config: &AppConfig) -> &str {
    match config.default_provider.as_str() {
        "ollama" => config.providers.ollama.default_model.as_str(),
        "openai_compat" => config.providers.openai_compat.default_model.as_str(),
        "copilot" => config.providers.copilot.default_model.as_str(),
        "openai_account" => config.providers.openai_account.default_model.as_str(),
        _ => "unknown",
    }
}

fn copilot_device_flow_config(config: &CopilotConfig) -> Result<GitHubDeviceFlowConfig, String> {
    let client_id = config
        .github_oauth_client_id
        .trim()
        .to_owned()
        .or_else_env([
            "ARTUI_GITHUB_OAUTH_CLIENT_ID",
            "GITHUB_COPILOT_OAUTH_CLIENT_ID",
        ]);
    let missing = [
        (
            "providers.copilot.github_oauth_client_id",
            client_id.as_str(),
        ),
        (
            "providers.copilot.github_device_code_url",
            config.github_device_code_url.as_str(),
        ),
        (
            "providers.copilot.github_oauth_token_url",
            config.github_oauth_token_url.as_str(),
        ),
    ]
    .into_iter()
    .filter_map(|(name, value)| value.trim().is_empty().then_some(name))
    .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "Configure {} before running /login copilot. You can set github_oauth_client_id in ~/.config/artui/config.toml or export ARTUI_GITHUB_OAUTH_CLIENT_ID.",
            missing.join(", ")
        ));
    }

    Ok(GitHubDeviceFlowConfig {
        provider_id: "copilot".to_owned(),
        client_id,
        device_code_url: config.github_device_code_url.clone(),
        token_url: config.github_oauth_token_url.clone(),
        scope: config.github_oauth_scope.clone(),
        timeout_secs: config.github_login_timeout_secs.max(1),
    })
}

trait EnvFallback {
    fn or_else_env<const N: usize>(self, names: [&str; N]) -> String;
}

impl EnvFallback for String {
    fn or_else_env<const N: usize>(self, names: [&str; N]) -> String {
        if !self.trim().is_empty() {
            return self;
        }
        names
            .into_iter()
            .find_map(|name| {
                std::env::var(name)
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            })
            .unwrap_or_default()
    }
}

fn available_model_options(config: &AppConfig, auth_store: Option<&AuthStore>) -> Vec<ModelOption> {
    let mut options = Vec::new();
    for provider in PROVIDERS {
        if !include_provider_models(config, auth_store, provider.id) {
            continue;
        }
        let models = provider_model_options(config, auth_store, provider.id);
        if models.is_empty() {
            continue;
        }
        options.push(ModelOption::header(provider.id, provider.display_name));
        for model in models {
            let hint = model_hint(config, auth_store, provider.id, &model);
            options.push(ModelOption::model(
                provider.id,
                provider.display_name,
                model,
                hint,
            ));
        }
    }
    options
}

fn include_provider_models(
    config: &AppConfig,
    auth_store: Option<&AuthStore>,
    provider_id: &str,
) -> bool {
    if provider_id == config.default_provider {
        return true;
    }

    match registry::provider_metadata(provider_id).map(|provider| provider.auth_requirement) {
        Some(AuthRequirement::None) => true,
        Some(AuthRequirement::ApiKey) => {
            std::env::var(config.providers.openai_compat.api_key_env.as_str())
                .ok()
                .filter(|value| !value.trim().is_empty())
                .is_some()
        }
        Some(AuthRequirement::Account) => auth_store
            .and_then(|store| store.status(provider_id).ok())
            .is_some_and(|status| status == AuthStatus::Connected),
        None => false,
    }
}

fn provider_model_options(
    config: &AppConfig,
    auth_store: Option<&AuthStore>,
    provider_id: &str,
) -> Vec<String> {
    match provider_id {
        "ollama" => unique_model_options(
            config.providers.ollama.default_model.as_str(),
            ollama_model_options(),
        ),
        "openai_compat" => unique_model_options(
            config.providers.openai_compat.default_model.as_str(),
            Vec::new(),
        ),
        "copilot" => unique_model_options(
            copilot_current_model(config, auth_store),
            copilot_discovered_models(config, auth_store),
        ),
        "openai_account" => unique_model_options(
            config.providers.openai_account.default_model.as_str(),
            Vec::new(),
        ),
        _ => Vec::new(),
    }
}

fn stored_copilot_models(auth_store: Option<&AuthStore>) -> Option<Vec<String>> {
    let record = auth_store?.record("copilot").ok()??;
    let models = record.metadata.get("models")?;
    serde_json::from_str::<Vec<String>>(models).ok()
}

fn model_hint(
    _config: &AppConfig,
    auth_store: Option<&AuthStore>,
    provider_id: &str,
    model: &str,
) -> Option<String> {
    match provider_id {
        "copilot" => copilot_model_hint(auth_store, model),
        "ollama" => Some("local".to_owned()),
        "openai_compat" => Some("api key".to_owned()),
        "openai_account" => Some("account".to_owned()),
        _ => None,
    }
}

fn copilot_model_hint(auth_store: Option<&AuthStore>, model: &str) -> Option<String> {
    let endpoint = stored_copilot_model_endpoint(auth_store, model)?;
    Some(endpoint.label())
}

fn copilot_model_context_window(auth_store: Option<&AuthStore>, model: &str) -> Option<usize> {
    stored_copilot_model_endpoint(auth_store, model)?.context_window_tokens
}

fn copilot_model_reasoning_efforts(
    auth_store: Option<&AuthStore>,
    model: &str,
) -> Option<Vec<ReasoningEffort>> {
    let endpoint = stored_copilot_model_endpoint(auth_store, model)?;
    if endpoint.reasoning_efforts.is_empty() {
        return None;
    }
    let mut efforts = vec![ReasoningEffort::Auto];
    for effort in endpoint.reasoning_efforts {
        let effort = match effort.as_str() {
            "low" => ReasoningEffort::Low,
            "medium" => ReasoningEffort::Medium,
            "high" => ReasoningEffort::High,
            "xhigh" => ReasoningEffort::XHigh,
            _ => continue,
        };
        if !efforts.contains(&effort) {
            efforts.push(effort);
        }
    }
    Some(efforts)
}

fn stored_copilot_model_endpoint(
    auth_store: Option<&AuthStore>,
    model: &str,
) -> Option<StoredModelEndpoint> {
    let record = auth_store?.record("copilot").ok()??;
    let metadata = record.metadata.get("model_endpoints")?;
    let endpoints = serde_json::from_str::<Vec<StoredModelEndpoint>>(metadata).ok()?;
    endpoints.into_iter().find(|endpoint| endpoint.id == model)
}

#[derive(Debug, serde::Deserialize)]
struct StoredModelEndpoint {
    id: String,
    api: String,
    #[serde(default)]
    supported_endpoints: Vec<String>,
    #[serde(default)]
    reasoning_efforts: Vec<String>,
    #[serde(default)]
    context_window_tokens: Option<usize>,
}

impl StoredModelEndpoint {
    fn label(&self) -> String {
        let mut labels = Vec::new();
        match self.api.as_str() {
            "responses" => labels.push("responses"),
            "messages" => labels.push("messages"),
            _ => labels.push("chat"),
        }
        if self.id.contains("codex") || self.id.starts_with("gpt-5") {
            labels.push("reasoning");
        }
        if self
            .supported_endpoints
            .iter()
            .any(|endpoint| endpoint.contains("embeddings"))
        {
            labels.push("embeddings");
        }
        labels.join(" • ")
    }
}

fn copilot_current_model<'a>(config: &'a AppConfig, auth_store: Option<&AuthStore>) -> &'a str {
    let current = config.providers.copilot.default_model.as_str();
    let discovered = copilot_discovered_models(config, auth_store);
    if !discovered.is_empty() && !discovered.iter().any(|model| model == current) {
        return "";
    }
    current
}

fn copilot_discovered_models(config: &AppConfig, auth_store: Option<&AuthStore>) -> Vec<String> {
    stored_copilot_models(auth_store)
        .filter(|models| !models.is_empty())
        .unwrap_or_else(|| config.providers.copilot.models.clone())
}

fn first_selectable_model_index(options: &[ModelOption]) -> Option<usize> {
    options.iter().position(ModelOption::is_selectable)
}

fn next_selectable_model_index(options: &[ModelOption], cursor: usize) -> Option<usize> {
    if options.is_empty() {
        return None;
    }
    (1..=options.len())
        .map(|offset| (cursor + offset) % options.len())
        .find(|index| options[*index].is_selectable())
}

fn previous_selectable_model_index(options: &[ModelOption], cursor: usize) -> Option<usize> {
    if options.is_empty() {
        return None;
    }
    (1..=options.len())
        .map(|offset| (cursor + options.len() - offset) % options.len())
        .find(|index| options[*index].is_selectable())
}

fn unique_model_options(current: &str, discovered: Vec<String>) -> Vec<String> {
    let mut models = Vec::new();
    for model in std::iter::once(current.to_owned()).chain(discovered) {
        let model = model.trim();
        if !model.is_empty() && !models.iter().any(|known| known == model) {
            models.push(model.to_owned());
        }
    }
    models
}

fn ollama_model_options() -> Vec<String> {
    let output = Command::new("ollama").arg("list").output().ok();
    let Some(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next())
        .filter(|model| !model.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

const CHAT_SCROLL_STEP: u16 = 3;
const CHAT_PAGE_SCROLL_STEP: u16 = 10;

const LOGO: &str = "┌────────┐\n│  >_  ●●│\n└────────┘";

const EYE_IDLE: &[&str] = &[
    "●●", "●●", "●●", "●●", "●●", "●●", "●●", "●●", "●●", "●●", "●●", "●●", "●●", "●●", "●●", "◡◡",
    "●●", "●●", "●●", "●●", "●●", "●●", "●●", "●●", "●●", "●●", "●●", "●●", "●●", "●●", "◕◕", "●●",
    "●●", "●●", "◔◔", "●●", "●●", "●●", "◡◡", "●●", "●●", "●●", "●●", "●●", "●●",
];

const EYE_TYPING: &[&str] = &["●●", "··", "●●", "··", "●●", "··", "●●", "··"];

const EYE_THINKING: &[&str] = &[
    "◔◔", "●●", "◕◕", "●●", "◔◔", "●●", "◕◕", "●●", "⚆⚆", "●●", "◡◡", "●●",
];

const EYE_STREAMING: &[&str] = &["⚆⚆", "●●", "⚆⚆", "●●", "⚆⚆", "●●", "◡◡", "●●"];

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use super::*;

    fn temp_auth_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "artui-{name}-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ))
    }

    fn connected_store_with_models(path: PathBuf, models: &[&str]) -> AuthStore {
        let mut metadata = BTreeMap::new();
        metadata.insert("models".to_owned(), serde_json::to_string(models).unwrap());
        let store = AuthStore::new(path);
        store
            .upsert(AuthRecord {
                provider_id: "copilot".to_owned(),
                account_label: Some("test".to_owned()),
                access_token: Some("secret".to_owned()),
                refresh_token: None,
                expires_at: None,
                updated_at: 0,
                metadata,
            })
            .unwrap();
        store
    }

    #[test]
    fn context_window_is_provider_model_dependent() {
        assert_eq!(
            known_context_window_tokens("ollama", "gemma4:e2b"),
            Some(32_000)
        );
        assert_eq!(
            known_context_window_tokens("copilot", "gpt-5.2-codex"),
            Some(400_000)
        );
        assert_eq!(
            known_context_window_tokens("copilot", "gpt-4.1"),
            Some(1_000_000)
        );
    }

    #[test]
    fn provider_reasoning_efforts_are_model_dependent() {
        assert_eq!(
            provider_reasoning_efforts("ollama", "qwen3:latest"),
            ReasoningEffort::AUTO_ONLY.to_vec()
        );
        assert_eq!(
            provider_reasoning_efforts("copilot", "gpt-5"),
            ReasoningEffort::STANDARD.to_vec()
        );
        assert_eq!(
            provider_reasoning_efforts("copilot", "gpt-5.5"),
            ReasoningEffort::EXTENDED.to_vec()
        );
    }

    #[test]
    fn connected_copilot_is_listed_in_model_options() {
        let config = AppConfig::default();
        let path = temp_auth_path("connected-copilot-options");
        let store = connected_store_with_models(path.clone(), &["plan-model"]);

        let options = available_model_options(&config, Some(&store));

        assert!(options.iter().any(|option| option.provider_id == "copilot"
            && option.model.as_deref() == Some("plan-model")));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn stored_copilot_models_are_grouped_under_provider_header() {
        let config = AppConfig::default();
        let path = temp_auth_path("stored-copilot-models");
        let store = connected_store_with_models(path.clone(), &["claude-sonnet-4.5", "gpt-5"]);

        let options = available_model_options(&config, Some(&store));
        let copilot_header = options
            .iter()
            .position(|option| option.provider_id == "copilot" && option.model.is_none())
            .unwrap();

        assert_eq!(options[copilot_header].provider_name, "GitHub Copilot");
        assert!(options[copilot_header + 1..].iter().any(|option| {
            option.provider_id == "copilot" && option.model.as_deref() == Some("claude-sonnet-4.5")
        }));
        assert!(options[copilot_header + 1..].iter().any(|option| {
            option.provider_id == "copilot" && option.model.as_deref() == Some("gpt-5")
        }));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn selecting_copilot_model_switches_provider_and_model() {
        let mut config = AppConfig::default();
        let path = temp_auth_path("select-copilot-model");
        config.auth_storage_path = Some(path.clone());
        let _store = connected_store_with_models(path.clone(), &["plan-model"]);
        let provider = build_provider(&config).unwrap();
        let mut app = App::new(config, provider);

        app.open_model_picker();
        app.model_cursor = app
            .model_options
            .iter()
            .position(|option| {
                option.provider_id == "copilot" && option.model.as_deref() == Some("plan-model")
            })
            .unwrap();
        app.select_model();

        assert_eq!(app.config.default_provider, "copilot");
        assert_eq!(app.active_model(), "plan-model");
        let _ = std::fs::remove_file(path);
    }
}
