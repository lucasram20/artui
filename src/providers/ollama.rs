use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::{
    app::{AppEvent, Role},
    config::OllamaConfig,
};

use super::{LlmProvider, ModelEvent, ModelRequest};

pub struct OllamaProvider {
    client: reqwest::Client,
    config: OllamaConfig,
}

impl OllamaProvider {
    pub fn new(config: OllamaConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    async fn send_event(tx: &mpsc::Sender<AppEvent>, event: ModelEvent) {
        let _ = tx.send(AppEvent::Model(event)).await;
    }

    async fn stream_chat(&self, request: ModelRequest, tx: mpsc::Sender<AppEvent>) -> Result<()> {
        let messages = request
            .messages
            .into_iter()
            .filter(|message| !message.content.is_empty())
            .map(|message| OllamaMessage {
                role: match message.role {
                    Role::User => "user".to_owned(),
                    Role::Assistant => "assistant".to_owned(),
                },
                content: message.content,
            })
            .collect();

        let body = OllamaChatRequest {
            model: self.config.default_model.clone(),
            messages,
            stream: true,
        };
        let response = self
            .client
            .post(format!(
                "{}/api/chat",
                self.config.host.trim_end_matches('/')
            ))
            .json(&body)
            .send()
            .await
            .context("failed to connect to Ollama")?
            .error_for_status()
            .context("Ollama returned an error")?;

        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("failed to read Ollama response")?;
            for line in String::from_utf8_lossy(&chunk).lines() {
                if line.trim().is_empty() {
                    continue;
                }

                let event: OllamaChatResponse =
                    serde_json::from_str(line).context("failed to parse Ollama stream event")?;
                if let Some(message) = event.message {
                    Self::send_event(&tx, ModelEvent::Token(message.content)).await;
                }
                if event.done {
                    Self::send_event(&tx, ModelEvent::Done).await;
                    return Ok(());
                }
            }
        }

        Self::send_event(&tx, ModelEvent::Done).await;
        Ok(())
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn stream_turn(&self, request: ModelRequest, tx: mpsc::Sender<AppEvent>) {
        if let Err(error) = self.stream_chat(request, tx.clone()).await {
            Self::send_event(&tx, ModelEvent::Error(error.to_string())).await;
        }
    }
}

#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: Option<OllamaResponseMessage>,
    done: bool,
}

#[derive(Debug, Deserialize)]
struct OllamaResponseMessage {
    content: String,
}
