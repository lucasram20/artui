//! Account-backed provider: streams from the ChatGPT backend Responses
//! API using the OAuth tokens persisted by `crate::auth::openai_oauth`.
//!
//! Endpoint:    `https://chatgpt.com/backend-api/codex/responses`
//! Auth:        `Authorization: Bearer <access_token>` from AuthStore
//! Account:     `ChatGPT-Account-Id: <account_id>` from id_token claims
//! Identity:    `OAI-Product-Sku: codex` (matches Codex CLI)
//! Wire format: OpenAI Responses API SSE — `response.output_text.delta`,
//!              `response.completed`, `response.failed`, etc.
//!
//! Tool calls are NOT yet wired through this provider — text-only first
//! iteration. Tool-call streaming will land in a follow-up alongside the
//! Responses API tool-event parser.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::{
    app::{AppEvent, Role},
    auth::{AuthRecord, AuthStore},
    config::OpenAiAccountConfig,
};

use super::{LlmProvider, ModelEvent, ModelRequest};

const PRODUCT_SKU: &str = "codex";
const REFRESH_LEEWAY_SECS: u64 = 60;

pub struct AccountProvider {
    provider_id: String,
    config: OpenAiAccountConfig,
    store: Option<AuthStore>,
    http: reqwest::Client,
}

impl AccountProvider {
    pub fn new(
        provider_id: impl Into<String>,
        config: OpenAiAccountConfig,
        store: Option<AuthStore>,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            config,
            store,
            http: reqwest::Client::builder()
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    async fn send_event(tx: &mpsc::Sender<AppEvent>, event: ModelEvent) {
        let _ = tx.send(AppEvent::Model(event)).await;
    }

    fn base_url(&self) -> String {
        let trimmed = self.config.base_url.trim().trim_end_matches('/');
        if trimmed.is_empty() {
            "https://chatgpt.com/backend-api/codex".to_owned()
        } else {
            trimmed.to_owned()
        }
    }

    fn default_model(&self) -> &str {
        let configured = self.config.default_model.trim();
        if configured.is_empty() {
            "gpt-5.3-codex"
        } else {
            configured
        }
    }

    async fn current_credentials(&self) -> Result<Credentials> {
        let store = self
            .store
            .as_ref()
            .context("auth store unavailable on this platform")?;
        let mut record = store
            .record(&self.provider_id)?
            .with_context(|| format!("no credentials saved; run /login {}", self.provider_id))?;

        if Self::should_refresh(&record) {
            self.refresh_into(store, &mut record).await?;
        }
        let access_token = record
            .access_token
            .clone()
            .filter(|t| !t.trim().is_empty())
            .with_context(|| {
                format!(
                    "stored {} credentials are missing access_token; run /login {}",
                    self.provider_id, self.provider_id
                )
            })?;
        let account_id = record
            .metadata
            .get("chatgpt_account_id")
            .cloned()
            .filter(|v| !v.trim().is_empty());
        Ok(Credentials {
            access_token,
            account_id,
        })
    }

    fn should_refresh(record: &AuthRecord) -> bool {
        let Some(expires_at) = record.expires_at else {
            return false;
        };
        let now = unix_timestamp();
        now.saturating_add(REFRESH_LEEWAY_SECS) >= expires_at
    }

    async fn refresh_into(&self, store: &AuthStore, record: &mut AuthRecord) -> Result<()> {
        let refresh_token = record
            .refresh_token
            .clone()
            .filter(|t| !t.trim().is_empty())
            .context("stored credentials have no refresh_token; run /login again")?;
        let issuer = record
            .metadata
            .get("issuer")
            .cloned()
            .unwrap_or_else(|| crate::auth::openai_oauth::DEFAULT_ISSUER.to_owned());
        let configured_id = self.config.oauth_client_id.trim().to_owned();
        let client_id = if configured_id.is_empty() {
            crate::auth::openai_oauth::DEFAULT_CLIENT_ID.to_owned()
        } else {
            configured_id
        };
        let scope = record
            .metadata
            .get("scope")
            .cloned()
            .unwrap_or_else(|| crate::auth::openai_oauth::DEFAULT_SCOPE.to_owned());

        let tokens = crate::auth::openai_oauth::refresh_access_token(
            &self.http,
            &issuer,
            &client_id,
            &refresh_token,
            &scope,
        )
        .await?;

        record.access_token = Some(tokens.access_token.clone());
        if let Some(rt) = tokens.refresh_token.filter(|t| !t.trim().is_empty()) {
            record.refresh_token = Some(rt);
        }
        record.expires_at = tokens.expires_in.map(|s| unix_timestamp().saturating_add(s));
        store.upsert(record.clone())?;
        Ok(())
    }

    async fn stream_responses(
        &self,
        request: ModelRequest,
        tx: mpsc::Sender<AppEvent>,
    ) -> Result<()> {
        let creds = self.current_credentials().await?;
        let model = self.default_model().to_owned();
        let url = format!("{}/responses", self.base_url());

        let mut input: Vec<Value> = Vec::new();
        for msg in &request.messages {
            if msg.content.trim().is_empty() {
                continue;
            }
            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            let part_type = if role == "user" {
                "input_text"
            } else {
                "output_text"
            };
            input.push(json!({
                "role": role,
                "content": [{"type": part_type, "text": msg.content}],
            }));
        }

        let mut body = json!({
            "model": model,
            "input": input,
            "stream": true,
            "store": false,
        });
        if let Some(system) = request
            .system_prompt
            .as_ref()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
        {
            body["instructions"] = Value::String(system);
        }
        if let Some(effort) = request
            .reasoning_effort
            .as_ref()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
        {
            body["reasoning"] = json!({"effort": effort});
        }
        if let Some(max_out) = request.max_output_tokens {
            body["max_output_tokens"] = json!(max_out);
        }

        let mut req = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", creds.access_token))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .header("OAI-Product-Sku", PRODUCT_SKU)
            .header("User-Agent", user_agent());
        if let Some(account_id) = &creds.account_id {
            req = req.header("ChatGPT-Account-Id", account_id);
        }

        let response = req
            .json(&body)
            .send()
            .await
            .context("failed to call ChatGPT backend Responses API")?;
        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            bail!(
                "ChatGPT backend returned HTTP {status}: {}",
                truncate_body(&body_text)
            );
        }

        let mut stream = response.bytes_stream();
        let mut buffer: Vec<u8> = Vec::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("failed to read ChatGPT backend stream")?;
            buffer.extend_from_slice(&chunk);
            while let Some(pos) = find_event_boundary(&buffer) {
                let event_bytes: Vec<u8> = buffer.drain(..pos).collect();
                buffer.drain(..2); // strip the "\n\n" delimiter
                let event_text = String::from_utf8_lossy(&event_bytes);
                if let Some(true) = handle_sse_block(&tx, &event_text).await? {
                    return Ok(());
                }
            }
        }

        if !buffer.is_empty() {
            let event_text = String::from_utf8_lossy(&buffer);
            handle_sse_block(&tx, &event_text).await?;
        }

        Self::send_event(&tx, ModelEvent::Done { end_turn: true }).await;
        Ok(())
    }
}

#[async_trait]
impl LlmProvider for AccountProvider {
    async fn stream_turn(&self, request: ModelRequest, tx: mpsc::Sender<AppEvent>) {
        if let Err(error) = self.stream_responses(request, tx.clone()).await {
            Self::send_event(&tx, ModelEvent::Error(error.to_string())).await;
        }
    }
}

#[derive(Debug, Clone)]
struct Credentials {
    access_token: String,
    account_id: Option<String>,
}

fn user_agent() -> String {
    format!("artui/{}", env!("CARGO_PKG_VERSION"))
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn truncate_body(body: &str) -> String {
    const CAP: usize = 512;
    if body.len() <= CAP {
        body.to_owned()
    } else {
        format!("{}…(truncated)", &body[..CAP])
    }
}

fn find_event_boundary(buffer: &[u8]) -> Option<usize> {
    buffer.windows(2).position(|w| w == b"\n\n")
}

async fn handle_sse_block(
    tx: &mpsc::Sender<AppEvent>,
    block: &str,
) -> Result<Option<bool>> {
    let mut data_lines: Vec<&str> = Vec::new();
    let mut event_kind: Option<&str> = None;
    for line in block.lines() {
        let line = line.trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            event_kind = Some(rest.trim());
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start());
        }
    }
    if data_lines.is_empty() {
        return Ok(None);
    }
    let data = data_lines.join("\n");
    if data == "[DONE]" {
        AccountProvider::send_event(tx, ModelEvent::Done { end_turn: true }).await;
        return Ok(Some(true));
    }
    let payload: Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    let kind = event_kind
        .map(str::to_owned)
        .or_else(|| {
            payload
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default();

    match kind.as_str() {
        "response.output_text.delta" | "response.refusal.delta" => {
            if let Some(delta) = payload.get("delta").and_then(Value::as_str) {
                if !delta.is_empty() {
                    AccountProvider::send_event(tx, ModelEvent::TextDelta(delta.to_owned())).await;
                }
            }
        }
        "response.reasoning_summary.delta" | "response.reasoning.delta" => {
            if let Some(delta) = payload.get("delta").and_then(Value::as_str) {
                if !delta.is_empty() {
                    AccountProvider::send_event(
                        tx,
                        ModelEvent::ReasoningDelta(delta.to_owned()),
                    )
                    .await;
                }
            }
        }
        "response.completed" => {
            if let Some(usage) = payload.pointer("/response/usage") {
                if let (Some(i), Some(o)) = (
                    usage.get("input_tokens").and_then(Value::as_u64),
                    usage.get("output_tokens").and_then(Value::as_u64),
                ) {
                    AccountProvider::send_event(
                        tx,
                        ModelEvent::Usage {
                            input_tokens: i as u32,
                            output_tokens: o as u32,
                        },
                    )
                    .await;
                }
            }
            AccountProvider::send_event(tx, ModelEvent::Done { end_turn: true }).await;
            return Ok(Some(true));
        }
        "response.failed" | "response.incomplete" | "error" => {
            let message = payload
                .pointer("/response/error/message")
                .and_then(Value::as_str)
                .or_else(|| payload.pointer("/error/message").and_then(Value::as_str))
                .or_else(|| payload.get("message").and_then(Value::as_str))
                .unwrap_or("ChatGPT backend stream failed")
                .to_owned();
            AccountProvider::send_event(tx, ModelEvent::Error(message)).await;
            return Ok(Some(true));
        }
        _ => {}
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn parses_output_text_delta_events() {
        let (tx, mut rx) = mpsc::channel(8);
        let block =
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"Hel\"}";
        handle_sse_block(&tx, block).await.unwrap();
        match rx.recv().await.unwrap() {
            AppEvent::Model(ModelEvent::TextDelta(text)) => assert_eq!(text, "Hel"),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn completed_event_emits_usage_and_done() {
        let (tx, mut rx) = mpsc::channel(8);
        let block = "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":17,\"output_tokens\":42}}}";
        let done = handle_sse_block(&tx, block).await.unwrap();
        assert_eq!(done, Some(true));
        let usage = rx.recv().await.unwrap();
        assert!(matches!(
            usage,
            AppEvent::Model(ModelEvent::Usage {
                input_tokens: 17,
                output_tokens: 42
            })
        ));
        let done_evt = rx.recv().await.unwrap();
        assert!(matches!(
            done_evt,
            AppEvent::Model(ModelEvent::Done { end_turn: true })
        ));
    }

    #[tokio::test]
    async fn failure_event_emits_error_and_terminates() {
        let (tx, mut rx) = mpsc::channel(8);
        let block = "event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"upstream rate limit\"}}}";
        let done = handle_sse_block(&tx, block).await.unwrap();
        assert_eq!(done, Some(true));
        match rx.recv().await.unwrap() {
            AppEvent::Model(ModelEvent::Error(msg)) => assert!(msg.contains("rate limit")),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn done_sentinel_ends_stream() {
        let (tx, mut rx) = mpsc::channel(8);
        let block = "data: [DONE]";
        let done = handle_sse_block(&tx, block).await.unwrap();
        assert_eq!(done, Some(true));
        let evt = rx.recv().await.unwrap();
        assert!(matches!(
            evt,
            AppEvent::Model(ModelEvent::Done { end_turn: true })
        ));
    }

    #[tokio::test]
    async fn ignores_non_json_data_lines() {
        let (tx, _rx) = mpsc::channel(8);
        let block = "data: keep-alive";
        let result = handle_sse_block(&tx, block).await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn reasoning_delta_emits_reasoning_event() {
        let (tx, mut rx) = mpsc::channel(8);
        let block = "event: response.reasoning_summary.delta\ndata: {\"type\":\"response.reasoning_summary.delta\",\"delta\":\"thinking…\"}";
        handle_sse_block(&tx, block).await.unwrap();
        match rx.recv().await.unwrap() {
            AppEvent::Model(ModelEvent::ReasoningDelta(text)) => assert_eq!(text, "thinking…"),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn boundary_finds_double_newline() {
        let buf = b"event: x\ndata: 1\n\nevent: y\ndata: 2\n\n";
        let pos = find_event_boundary(buf).unwrap();
        assert_eq!(&buf[..pos], b"event: x\ndata: 1");
    }
}
