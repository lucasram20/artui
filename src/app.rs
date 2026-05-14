use std::{process::Command, sync::Arc};

use crate::{
    config::AppConfig,
    providers::{build_provider, LlmProvider, ModelEvent, ModelRequest},
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
}

impl ThemeId {
    pub const ALL: [Self; 6] = [
        Self::MonokaiBlue,
        Self::TokyoNight,
        Self::CatppuccinMocha,
        Self::Gruvbox,
        Self::Nord,
        Self::Dracula,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::MonokaiBlue => "Monokai Blue",
            Self::TokyoNight => "Tokyo Night",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::Gruvbox => "Gruvbox",
            Self::Nord => "Nord",
            Self::Dracula => "Dracula",
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

#[derive(Debug)]
pub enum AppEvent {
    Model(ModelEvent),
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

pub const SLASH_COMMANDS: [SlashCommand; 7] = [
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLineItem {
    Model,
    CurrentDir,
    ProjectName,
    GitBranch,
    Context,
    GitStatus,
    EscHint,
}

impl StatusLineItem {
    pub const ALL: [Self; 7] = [
        Self::Model,
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

pub struct App {
    pub config: AppConfig,
    pub provider: Arc<dyn LlmProvider>,
    pub mode: UiMode,
    pub transcript: Vec<Message>,
    pub input: String,
    pub status: String,
    pub should_quit: bool,
    pub slash_cursor: usize,
    pub chat_scroll: u16,
    pub logo: &'static str,
    pub theme: ThemeId,
    pub theme_picker_open: bool,
    pub theme_cursor: usize,
    pub model_picker_open: bool,
    pub model_cursor: usize,
    pub model_options: Vec<String>,
    pub statusline_open: bool,
    pub statusline_cursor: usize,
    pub statusline_enabled: [bool; StatusLineItem::ALL.len()],
    pub git_branch_label: String,
    pub git_status_label: String,
}

impl App {
    pub fn new(config: AppConfig, provider: Arc<dyn LlmProvider>) -> Self {
        let model_options = available_model_options(&config);
        Self {
            status: format!("Provider: {}", config.default_provider),
            config,
            provider,
            mode: UiMode::Input,
            transcript: Vec::new(),
            input: String::new(),
            should_quit: false,
            slash_cursor: 0,
            chat_scroll: 0,
            logo: LOGO,
            theme: ThemeId::MonokaiBlue,
            theme_picker_open: false,
            theme_cursor: ThemeId::MonokaiBlue.index(),
            model_picker_open: false,
            model_cursor: 0,
            model_options,
            statusline_open: false,
            statusline_cursor: 0,
            statusline_enabled: [true; StatusLineItem::ALL.len()],
            git_branch_label: git_branch().unwrap_or_else(|| "no-git".to_owned()),
            git_status_label: git_status_label().unwrap_or_else(|| "unknown".to_owned()),
        }
    }

    pub fn edit_input(&mut self, action: InputAction) {
        if self.mode == UiMode::Streaming
            || self.theme_picker_open
            || self.model_picker_open
            || self.statusline_open
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

    pub fn submit_input(&mut self) -> Option<ProviderRequest> {
        if self.mode == UiMode::Streaming {
            return None;
        }

        let content = self.input.trim().to_owned();
        if content.is_empty() {
            return None;
        }
        if self.run_slash_command(&content) {
            self.input.clear();
            self.slash_cursor = 0;
            return None;
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

        Some(ProviderRequest {
            provider: Arc::clone(&self.provider),
            request: ModelRequest {
                messages: self.transcript.clone(),
            },
        })
    }

    pub fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Model(ModelEvent::Token(token)) => self.append_assistant_token(&token),
            AppEvent::Model(ModelEvent::Done) => {
                self.mode = UiMode::Input;
                self.status = format!("Provider: {}", self.config.default_provider);
            }
            AppEvent::Model(ModelEvent::Error(error)) => {
                self.append_assistant_token(&format!("\nError: {error}"));
                self.mode = UiMode::Input;
                self.status = "Provider error".to_owned();
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
        if self.statusline_open {
            self.close_statusline_picker();
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
        self.status = format!("Provider: {}", self.config.default_provider);
    }

    pub fn open_statusline_picker(&mut self) {
        self.theme_picker_open = false;
        self.model_picker_open = false;
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
        self.status = format!("Provider: {}", self.config.default_provider);
    }

    pub fn open_model_picker(&mut self) {
        self.theme_picker_open = false;
        self.statusline_open = false;
        self.model_options = available_model_options(&self.config);
        let active_model = self.active_model().to_owned();
        self.model_cursor = self
            .model_options
            .iter()
            .position(|model| model == &active_model)
            .unwrap_or(0);
        self.model_picker_open = true;
        self.mode = UiMode::Normal;
        self.status = "Select a model".to_owned();
    }

    pub fn next_model(&mut self) {
        if self.model_picker_open && !self.model_options.is_empty() {
            self.model_cursor = (self.model_cursor + 1) % self.model_options.len();
        }
    }

    pub fn previous_model(&mut self) {
        if self.model_picker_open && !self.model_options.is_empty() {
            self.model_cursor =
                (self.model_cursor + self.model_options.len() - 1) % self.model_options.len();
        }
    }

    pub fn select_model(&mut self) {
        if !self.model_picker_open {
            return;
        }
        if let Some(model) = self.model_options.get(self.model_cursor).cloned() {
            self.switch_active_model(model);
        }
        self.model_picker_open = false;
        self.mode = UiMode::Input;
    }

    fn close_model_picker(&mut self) {
        self.model_picker_open = false;
        self.mode = UiMode::Input;
        self.status = format!("Model: {}", self.active_model());
    }

    pub fn active_model(&self) -> &str {
        active_model_from_config(&self.config)
    }

    fn switch_active_model(&mut self, model: String) {
        match self.config.default_provider.as_str() {
            "ollama" => self.config.providers.ollama.default_model = model.clone(),
            "openai_compat" => self.config.providers.openai_compat.default_model = model.clone(),
            provider => {
                self.status = format!("Cannot switch model for unsupported provider: {provider}");
                return;
            }
        }

        match build_provider(&self.config) {
            Ok(provider) => {
                self.provider = provider;
                self.status = format!("Model: {model}");
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
            && !self.statusline_open
            && !self.has_slash_command_matches()
    }

    pub fn complete_slash_command(&mut self) {
        if let Some(command) = self.selected_slash_command() {
            self.input = command.name.to_owned();
            self.mode = UiMode::Input;
        }
    }

    pub fn submit_slash_command_selection(&mut self) -> Option<ProviderRequest> {
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

    fn run_slash_command(&mut self, content: &str) -> bool {
        match content {
            "/help" => {
                self.show_help();
                true
            }
            "/theme" => {
                self.open_theme_picker();
                true
            }
            "/model" => {
                self.open_model_picker();
                true
            }
            "/statusline" => {
                self.open_statusline_picker();
                true
            }
            "/clear" => {
                self.clear_transcript();
                self.mode = UiMode::Input;
                true
            }
            "/quit" | "/exit" => {
                self.should_quit = true;
                true
            }
            command if command.starts_with("/model ") => {
                let model = command.trim_start_matches("/model").trim();
                if model.is_empty() {
                    self.open_model_picker();
                } else {
                    self.switch_active_model(model.to_owned());
                }
                true
            }
            command if command.starts_with('/') => {
                self.status = format!("Unknown command: {command}");
                true
            }
            _ => false,
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
            "working tree clean".to_owned()
        } else {
            format!("{} changed", status.lines().count())
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

fn active_model_from_config(config: &AppConfig) -> &str {
    match config.default_provider.as_str() {
        "ollama" => config.providers.ollama.default_model.as_str(),
        "openai_compat" => config.providers.openai_compat.default_model.as_str(),
        _ => "unknown",
    }
}

fn available_model_options(config: &AppConfig) -> Vec<String> {
    let current = active_model_from_config(config);
    let discovered = match config.default_provider.as_str() {
        "ollama" => ollama_model_options(),
        "openai_compat" => Vec::new(),
        _ => Vec::new(),
    };

    unique_model_options(current, discovered)
}

fn unique_model_options(current: &str, discovered: Vec<String>) -> Vec<String> {
    let mut models = Vec::new();
    for model in std::iter::once(current.to_owned()).chain(discovered) {
        if !model.is_empty() && !models.iter().any(|known| known == &model) {
            models.push(model);
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
