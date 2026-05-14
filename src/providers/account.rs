use anyhow::{bail, Result};
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::app::AppEvent;

use super::{LlmProvider, ModelEvent, ModelRequest};

pub struct AccountProvider {
    provider_id: String,
}

impl AccountProvider {
    pub fn new(provider_id: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
        }
    }
}

#[async_trait]
impl LlmProvider for AccountProvider {
    async fn stream_turn(&self, _request: ModelRequest, tx: mpsc::Sender<AppEvent>) {
        let result: Result<()> = async {
            bail!(
                "{} account streaming is not implemented yet. Use /login for connection status.",
                self.provider_id
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
