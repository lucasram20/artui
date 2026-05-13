use anyhow::{bail, Result};
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{app::AppEvent, config::OpenAiCompatConfig};

use super::{LlmProvider, ModelEvent, ModelRequest};

pub struct OpenAiCompatProvider {
    config: OpenAiCompatConfig,
}

impl OpenAiCompatProvider {
    pub fn new(config: OpenAiCompatConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    async fn stream_turn(&self, _request: ModelRequest, tx: mpsc::Sender<AppEvent>) {
        let result: Result<()> = async {
            bail!(
                "OpenAI-compatible streaming is not implemented yet for {}",
                self.config.base_url
            )
        }
        .await;

        if let Err(error) = result {
            let _ = tx
                .send(AppEvent::Model(ModelEvent::Error(error.to_string())))
                .await;
        }
    }
}
