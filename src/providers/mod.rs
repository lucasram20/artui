mod account;
pub(crate) mod copilot;
mod ollama;
mod openai_compat;
pub mod registry;

use std::sync::Arc;

use anyhow::{bail, Result};
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{app::Message, config::AppConfig};

pub use ollama::OllamaProvider;
pub use openai_compat::OpenAiCompatProvider;

#[derive(Debug, Clone)]
pub struct ModelRequest {
    pub messages: Vec<Message>,
    pub system_prompt: Option<String>,
    pub reasoning_effort: Option<String>,
}

#[derive(Debug)]
pub enum ModelEvent {
    Token(String),
    Done,
    Error(String),
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn stream_turn(&self, request: ModelRequest, tx: mpsc::Sender<crate::app::AppEvent>);
}

pub fn build_provider(config: &AppConfig) -> Result<Arc<dyn LlmProvider>> {
    match config.default_provider.as_str() {
        "ollama" => Ok(Arc::new(OllamaProvider::new(
            config.providers.ollama.clone(),
        ))),
        "openai_compat" => Ok(Arc::new(OpenAiCompatProvider::new(
            config.providers.openai_compat.clone(),
        ))),
        "copilot" => Ok(Arc::new(copilot::CopilotProvider::new(
            config.providers.copilot.clone(),
            crate::auth::AuthStore::from_config(config),
        ))),
        "openai_account" => Ok(Arc::new(account::AccountProvider::new(
            config.default_provider.clone(),
        ))),
        provider => bail!("unsupported provider: {provider}"),
    }
}
