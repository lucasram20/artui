use anyhow::{bail, Context, Result};
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

    async fn handle_stream_line(tx: &mpsc::Sender<AppEvent>, line: &str) -> Result<bool> {
        let event: OllamaChatResponse =
            serde_json::from_str(line).context("failed to parse Ollama stream event")?;
        if let Some(message) = event.message {
            Self::send_event(tx, ModelEvent::Token(message.content)).await;
        }
        if event.done {
            Self::send_event(tx, ModelEvent::Done).await;
            return Ok(true);
        }

        Ok(false)
    }

    async fn stream_chat(&self, request: ModelRequest, tx: mpsc::Sender<AppEvent>) -> Result<()> {
        let messages = request
            .system_prompt
            .into_iter()
            .filter(|content| !content.trim().is_empty())
            .map(|content| OllamaMessage {
                role: "system".to_owned(),
                content,
            })
            .chain(
                request
                    .messages
                    .into_iter()
                    .filter(|message| !message.content.is_empty())
                    .map(|message| OllamaMessage {
                        role: match message.role {
                            Role::User => "user".to_owned(),
                            Role::Assistant => "assistant".to_owned(),
                        },
                        content: message.content,
                    }),
            )
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
            .context("failed to connect to Ollama")?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .context("failed to read Ollama error response")?;
            bail!("Ollama returned HTTP {status}: {body}");
        }

        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("failed to read Ollama response")?;
            buffer.extend_from_slice(&chunk);

            while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
                let line = buffer.drain(..=newline).collect::<Vec<_>>();
                let line = String::from_utf8_lossy(&line);
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if Self::handle_stream_line(&tx, line).await? {
                    return Ok(());
                }
            }
        }

        let line = String::from_utf8_lossy(&buffer);
        let line = line.trim();
        if !line.is_empty() && Self::handle_stream_line(&tx, line).await? {
            return Ok(());
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
