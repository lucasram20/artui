mod account;
pub(crate) mod copilot;
mod ollama;
mod openai_compat;
pub mod registry;
pub mod tool_serialization;

use std::sync::Arc;

use anyhow::{bail, Result};
use async_trait::async_trait;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::{app::Message, config::AppConfig};

pub use ollama::OllamaProvider;
pub use openai_compat::OpenAiCompatProvider;

// ---------------------------------------------------------------------------
// Tool protocol types
// ---------------------------------------------------------------------------

/// Describes a tool the model may call.
#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool's parameters.
    pub parameters: serde_json::Value,
}

/// Controls whether/how the model should use tools.
#[derive(Debug, Clone, Default, Serialize)]
pub enum ToolChoice {
    #[default]
    Auto,
    Required,
    None,
    Specific(String),
}

/// A completed tool call extracted from the model stream.
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Model request / event
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ModelRequest {
    pub messages: Vec<Message>,
    pub system_prompt: Option<String>,
    pub reasoning_effort: Option<String>,
    /// Tools available for this turn. Empty = no tool use.
    pub tools: Vec<ToolSpec>,
    /// How the model should pick tools.
    pub tool_choice: ToolChoice,
    /// Optional cap on output tokens.
    pub max_output_tokens: Option<u32>,
}

#[derive(Debug, Clone)]
pub enum ModelEvent {
    /// Plain text delta from the assistant message.
    TextDelta(String),
    /// Model started a tool call.
    ToolCallStart { id: String, name: String },
    /// Streaming JSON args for an in-progress tool call.
    ToolCallArgsDelta { id: String, json_chunk: String },
    /// Tool call complete with assembled arguments.
    ToolCallEnd {
        id: String,
        arguments: serde_json::Value,
    },
    /// Model reasoning trace (chain-of-thought).
    ReasoningDelta(String),
    /// Token usage for the turn.
    Usage {
        input_tokens: u32,
        output_tokens: u32,
    },
    /// Stream finished.
    Done { end_turn: bool },
    /// Unrecoverable error.
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
