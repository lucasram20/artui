use std::{process::Command, sync::Arc};

use crate::{
    config::AppConfig,
    providers::{LlmProvider, ModelEvent, ModelRequest},
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
    pub logo: &'static str,
    pub theme: ThemeId,
    pub theme_picker_open: bool,
    pub theme_cursor: usize,
    pub git_branch_label: String,
    pub git_status_label: String,
}

impl App {
    pub fn new(config: AppConfig, provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            status: format!("Provider: {}", config.default_provider),
            config,
            provider,
            mode: UiMode::Input,
            transcript: Vec::new(),
            input: String::new(),
            should_quit: false,
            logo: LOGO,
            theme: ThemeId::MonokaiBlue,
            theme_picker_open: false,
            theme_cursor: ThemeId::MonokaiBlue.index(),
            git_branch_label: git_branch().unwrap_or_else(|| "no-git".to_owned()),
            git_status_label: git_status_label().unwrap_or_else(|| "unknown".to_owned()),
        }
    }

    pub fn edit_input(&mut self, action: InputAction) {
        if self.mode == UiMode::Streaming || self.theme_picker_open {
            return;
        }

        self.mode = UiMode::Input;
        match action {
            InputAction::Insert(ch) => self.input.push(ch),
            InputAction::Backspace => {
                self.input.pop();
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
        if content == "/theme" {
            self.input.clear();
            self.open_theme_picker();
            return None;
        }

        self.input.clear();
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
        if self.mode != UiMode::Streaming {
            self.input.clear();
            self.mode = UiMode::Normal;
        }
    }

    pub fn open_theme_picker(&mut self) {
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

    pub fn clear_transcript(&mut self) {
        self.transcript.clear();
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

const LOGO: &str = "┌────────┐\n│  >_  ●●│\n└────────┘";
