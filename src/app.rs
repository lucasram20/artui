use std::sync::Arc;

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
        }
    }

    pub fn edit_input(&mut self, action: InputAction) {
        if self.mode == UiMode::Streaming {
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
        if self.mode != UiMode::Streaming {
            self.input.clear();
            self.mode = UiMode::Normal;
        }
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
