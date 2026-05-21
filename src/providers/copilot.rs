use std::{
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::{
    app::{AppEvent, Role},
    auth::AuthStore,
    config::CopilotConfig,
};

use super::{LlmProvider, ModelEvent, ModelRequest};

pub struct CopilotProvider {
    client: reqwest::Client,
    config: CopilotConfig,
    store: Option<AuthStore>,
}

impl CopilotProvider {
    pub fn new(config: CopilotConfig, store: Option<AuthStore>) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
            store,
        }
    }

    async fn send_event(tx: &mpsc::Sender<AppEvent>, event: ModelEvent) {
        let _ = tx.send(AppEvent::Model(event)).await;
    }

    async fn stream_chat(&self, request: ModelRequest, tx: mpsc::Sender<AppEvent>) -> Result<()> {
        let model = self.active_model();
        self.validate_active_model(&model)?;
        let session = self.session_for_model(&model, false).await?;
        let response = match self
            .send_request_for_api(request.clone(), &session.session, &model, session.api)
            .await
        {
            Ok(response) => response,
            Err(error) if error.is_unauthorized_or_expired() => {
                let refreshed = self.session_for_model(&model, true).await?;
                self.send_request_for_api(request, &refreshed.session, &model, refreshed.api)
                    .await
                    .map_err(anyhow::Error::from)?
            }
            Err(error) => {
                self.handle_model_error(error, request, &session.session)
                    .await?
            }
        };

        stream_sse_response(response, &tx).await
    }

    async fn send_request_for_api(
        &self,
        request: ModelRequest,
        session: &CopilotSession,
        model: &str,
        api: CopilotApiKind,
    ) -> Result<reqwest::Response, CopilotRequestError> {
        match api {
            CopilotApiKind::Messages => self.send_messages_request(request, session, model).await,
            CopilotApiKind::Responses => match self
                .send_responses_request(request.clone(), session, model)
                .await
            {
                Ok(response) => Ok(response),
                Err(error) if error.is_unsupported_api_for_model() => {
                    self.send_chat_request(request, session, model).await
                }
                Err(error) => Err(error),
            },
            CopilotApiKind::Chat => match self
                .send_chat_request(request.clone(), session, model)
                .await
            {
                Ok(response) => Ok(response),
                Err(error) if error.is_unsupported_api_for_model() => {
                    self.send_responses_request(request, session, model).await
                }
                Err(error) => Err(error),
            },
        }
    }

    async fn handle_model_error(
        &self,
        error: CopilotRequestError,
        request: ModelRequest,
        session: &CopilotSession,
    ) -> Result<reqwest::Response> {
        if error.is_model_not_supported() {
            let models = fetch_models_with_session(&self.client, &self.config, session).await?;
            self.save_discovered_models(&models)?;
            let Some(model) = models.into_iter().next() else {
                bail!("GitHub Copilot did not return any models for this account");
            };
            return match model.api_kind() {
                CopilotApiKind::Messages => self
                    .send_messages_request(request, session, &model.id)
                    .await
                    .map_err(Into::into),
                CopilotApiKind::Responses => self
                    .send_responses_request(request, session, &model.id)
                    .await
                    .map_err(Into::into),
                CopilotApiKind::Chat => self
                    .send_chat_request(request, session, &model.id)
                    .await
                    .map_err(Into::into),
            };
        }
        bail!("{error}")
    }

    async fn send_messages_request(
        &self,
        request: ModelRequest,
        session: &CopilotSession,
        model: &str,
    ) -> Result<reqwest::Response, CopilotRequestError> {
        let system = request
            .system_prompt
            .clone()
            .filter(|content| !content.trim().is_empty());
        let messages = copilot_conversation_messages(request);

        let body = CopilotMessagesRequest {
            model: model.to_owned(),
            messages,
            system,
            max_tokens: 4096,
            stream: true,
        };
        let mut headers = copilot_api_headers(&self.config, &session.token).map_err(|error| {
            CopilotRequestError {
                status: None,
                body: error.to_string(),
            }
        })?;
        headers.insert(
            "anthropic-version",
            reqwest::header::HeaderValue::from_static("2023-06-01"),
        );

        let response = self
            .client
            .post(format!(
                "{}/v1/messages",
                session.api_base_url.trim_end_matches('/')
            ))
            .headers(headers)
            .json(&body)
            .timeout(request_timeout(&self.config))
            .send()
            .await
            .map_err(|source| CopilotRequestError {
                status: None,
                body: source.to_string(),
            })?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_else(|error| {
                format!("failed to read GitHub Copilot error response: {error}")
            });
            return Err(CopilotRequestError {
                status: Some(status),
                body,
            });
        }

        Ok(response)
    }

    async fn send_chat_request(
        &self,
        request: ModelRequest,
        session: &CopilotSession,
        model: &str,
    ) -> Result<reqwest::Response, CopilotRequestError> {
        let reasoning_effort = request.reasoning_effort.clone();
        let messages = copilot_chat_messages(request);

        let body = CopilotChatRequest {
            model: model.to_owned(),
            messages,
            reasoning_effort,
            stream: true,
        };
        let response = self
            .client
            .post(format!(
                "{}/chat/completions",
                session.api_base_url.trim_end_matches('/')
            ))
            .headers(
                copilot_api_headers(&self.config, &session.token).map_err(|error| {
                    CopilotRequestError {
                        status: None,
                        body: error.to_string(),
                    }
                })?,
            )
            .json(&body)
            .timeout(request_timeout(&self.config))
            .send()
            .await
            .map_err(|source| CopilotRequestError {
                status: None,
                body: source.to_string(),
            })?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_else(|error| {
                format!("failed to read GitHub Copilot error response: {error}")
            });
            return Err(CopilotRequestError {
                status: Some(status),
                body,
            });
        }

        Ok(response)
    }

    async fn send_responses_request(
        &self,
        request: ModelRequest,
        session: &CopilotSession,
        model: &str,
    ) -> Result<reqwest::Response, CopilotRequestError> {
        let instructions = request
            .system_prompt
            .clone()
            .filter(|content| !content.trim().is_empty());
        let reasoning = request
            .reasoning_effort
            .clone()
            .map(|effort| CopilotReasoning { effort });
        let input = copilot_response_input(request);

        let body = CopilotResponsesRequest {
            model: model.to_owned(),
            input,
            instructions,
            reasoning,
            stream: true,
        };
        let response = self
            .client
            .post(format!(
                "{}/responses",
                session.api_base_url.trim_end_matches('/')
            ))
            .headers(
                copilot_api_headers(&self.config, &session.token).map_err(|error| {
                    CopilotRequestError {
                        status: None,
                        body: error.to_string(),
                    }
                })?,
            )
            .json(&body)
            .timeout(request_timeout(&self.config))
            .send()
            .await
            .map_err(|source| CopilotRequestError {
                status: None,
                body: source.to_string(),
            })?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_else(|error| {
                format!("failed to read GitHub Copilot error response: {error}")
            });
            return Err(CopilotRequestError {
                status: Some(status),
                body,
            });
        }

        Ok(response)
    }

    fn active_model(&self) -> String {
        let configured = self.config.default_model.trim();
        let discovered = self
            .store
            .as_ref()
            .and_then(|store| store.record("copilot").ok().flatten())
            .and_then(|record| record.metadata.get("models").cloned())
            .and_then(|models| serde_json::from_str::<Vec<String>>(&models).ok());

        if let Some(discovered) = discovered {
            if discovered
                .iter()
                .any(|model| model == &self.config.default_model)
            {
                return self.config.default_model.clone();
            }
            return discovered.into_iter().next().unwrap_or_default();
        }

        configured.to_owned()
    }

    async fn session_for_model(
        &self,
        model: &str,
        force_refresh: bool,
    ) -> Result<ResolvedCopilotSession> {
        let candidates = token_candidates(&self.config, self.store.as_ref())?;
        let mut fallback = None;
        let mut last_error = None;
        for candidate in candidates {
            match exchange_token_candidate(
                &self.client,
                &self.config,
                self.store.as_ref(),
                &candidate,
                force_refresh,
            )
            .await
            {
                Ok(session) => {
                    match fetch_models_with_session(&self.client, &self.config, &session).await {
                        Ok(models) => {
                            if let Some(api) = models
                                .iter()
                                .find(|known| known.id == model)
                                .map(CopilotModel::api_kind)
                            {
                                self.save_discovered_models(&models)?;
                                return Ok(ResolvedCopilotSession { session, api });
                            }
                            if fallback.as_ref().is_none_or(
                                |(_, known): &(CopilotSession, Vec<CopilotModel>)| {
                                    models.len() > known.len()
                                },
                            ) {
                                fallback = Some((session, models));
                            }
                        }
                        Err(error) => last_error = Some(format!("{}: {error}", candidate.label)),
                    }
                }
                Err(error) => last_error = Some(format!("{}: {error}", candidate.label)),
            }
        }

        if let Some((session, models)) = fallback {
            self.save_discovered_models(&models)?;
            let api = models
                .first()
                .map(CopilotModel::api_kind)
                .unwrap_or(CopilotApiKind::Chat);
            return Ok(ResolvedCopilotSession { session, api });
        }

        bail!(
            "No GitHub Copilot token could fetch models. {}",
            last_error.unwrap_or_else(
                || "Run /login copilot or configure GH_TOKEN/GITHUB_TOKEN.".to_owned()
            )
        )
    }

    fn validate_active_model(&self, model: &str) -> Result<()> {
        if !model.trim().is_empty() {
            return Ok(());
        }
        bail!(
            "GitHub Copilot models are not available yet. Open /model while connected or run /login copilot again so artui can fetch the models for your plan."
        )
    }

    fn save_discovered_models(&self, models: &[CopilotModel]) -> Result<()> {
        let Some(store) = &self.store else {
            return Ok(());
        };
        if let Some(mut record) = store.record("copilot")? {
            let ids = models
                .iter()
                .map(|model| model.id.clone())
                .collect::<Vec<_>>();
            record
                .metadata
                .insert("models".to_owned(), serde_json::to_string(&ids)?);
            record.metadata.insert(
                "model_endpoints".to_owned(),
                serde_json::to_string(&model_endpoint_metadata(models))?,
            );
            store.upsert(record)?;
        }
        Ok(())
    }
}

#[async_trait]
impl LlmProvider for CopilotProvider {
    async fn stream_turn(&self, request: ModelRequest, tx: mpsc::Sender<AppEvent>) {
        if let Err(error) = self.stream_chat(request, tx.clone()).await {
            Self::send_event(&tx, ModelEvent::Error(error.to_string())).await;
        }
    }
}

pub async fn fetch_copilot_models(
    config: &CopilotConfig,
    store: &AuthStore,
) -> Result<Vec<String>> {
    let client = reqwest::Client::new();
    let candidates = token_candidates(config, Some(store))?;
    let mut selected = None;
    let mut last_error = None;
    for candidate in candidates {
        match exchange_token_candidate(&client, config, Some(store), &candidate, false).await {
            Ok(session) => match fetch_models_with_session(&client, config, &session).await {
                Ok(models) => {
                    if selected.as_ref().is_none_or(
                        |(_, known): &(TokenCandidate, Vec<CopilotModel>)| {
                            models.len() > known.len()
                        },
                    ) {
                        selected = Some((candidate, models));
                    }
                }
                Err(error) => last_error = Some(format!("{}: {error}", candidate.label)),
            },
            Err(error) => last_error = Some(format!("{}: {error}", candidate.label)),
        }
    }

    let (candidate, models) = selected.with_context(|| {
        format!(
            "No GitHub Copilot token could fetch models. {}",
            last_error.unwrap_or_else(|| {
                "Run /login copilot or configure GH_TOKEN/GITHUB_TOKEN.".to_owned()
            })
        )
    })?;
    if let Some(mut record) = store.record("copilot")? {
        record.metadata.insert(
            "model_endpoints".to_owned(),
            serde_json::to_string(&model_endpoint_metadata(&models))?,
        );
        record
            .metadata
            .insert("model_source".to_owned(), candidate.label.clone());
        if let Some(usage) = fetch_copilot_usage(&client, config, &candidate)
            .await
            .ok()
            .flatten()
        {
            record
                .metadata
                .insert("usage_label".to_owned(), usage.label());
        }
        store.upsert(record)?;
    }
    Ok(models.into_iter().map(|model| model.id).collect::<Vec<_>>())
}

async fn fetch_copilot_usage(
    client: &reqwest::Client,
    config: &CopilotConfig,
    candidate: &TokenCandidate,
) -> Result<Option<CopilotUsage>> {
    let response = client
        .get("https://api.github.com/copilot_internal/user")
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::USER_AGENT, user_agent())
        .header("X-GitHub-Api-Version", "2025-05-01")
        .bearer_auth(&candidate.token)
        .timeout(request_timeout(config))
        .send()
        .await?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let usage = response
        .json::<CopilotUsageResponse>()
        .await
        .context("failed to parse GitHub Copilot usage response")?;
    Ok(CopilotUsage::from_response(usage))
}

async fn exchange_token_candidate(
    client: &reqwest::Client,
    config: &CopilotConfig,
    store: Option<&AuthStore>,
    candidate: &TokenCandidate,
    force_refresh: bool,
) -> Result<CopilotSession> {
    if candidate.cacheable && !force_refresh {
        if let Some(session) = cached_copilot_session(store)? {
            return Ok(session);
        }
    }

    let session = exchange_github_token(client, config, &candidate.token).await?;
    if candidate.cacheable {
        save_cached_copilot_session(store, &session)?;
    }
    Ok(session)
}

async fn exchange_github_token(
    client: &reqwest::Client,
    config: &CopilotConfig,
    github_token: &str,
) -> Result<CopilotSession> {
    validate_https_url(&config.token_url, "providers.copilot.token_url")?;
    let response = client
        .get(config.token_url.trim())
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::USER_AGENT, user_agent())
        .header(
            reqwest::header::AUTHORIZATION,
            format!("token {github_token}"),
        )
        .timeout(request_timeout(config))
        .send()
        .await
        .context("failed to exchange GitHub token for a Copilot session token")?;
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .context("failed to read GitHub Copilot token exchange error")?;
        // Reject PATs (ghp_) — they don't work with Copilot's /chat/completions endpoint.
        if github_token.starts_with("ghp_") {
            bail!(
                "Personal Access Tokens (ghp_) are not supported for GitHub Copilot. \
                 Run `/login copilot` to authenticate via device flow."
            );
        }
        // For device-flow tokens (ghu_) and other OAuth tokens: if token exchange
        // fails (e.g. 404 on student/free plans), use the token directly as Bearer
        // auth against the Copilot API — this matches opencode's approach.
        if github_token.starts_with("ghu_") || !github_token.starts_with("ghp_") {
            tracing::debug!("Copilot token exchange returned {status}; using OAuth token directly");
            validate_https_url(&config.api_base_url, "providers.copilot.api_base_url")?;
            return Ok(CopilotSession {
                token: github_token.to_owned(),
                api_base_url: config.api_base_url.clone(),
                expires_at: None,
            });
        }
        bail!(
            "GitHub Copilot token exchange returned HTTP {status}: {}",
            truncate_body(&body)
        );
    }

    let token: CopilotTokenResponse = response
        .json()
        .await
        .context("failed to parse GitHub Copilot token exchange response")?;
    let session_token = token
        .token
        .filter(|value| !value.trim().is_empty())
        .context("GitHub Copilot token exchange response did not include a token")?;
    let api_base_url = token
        .endpoints
        .and_then(|endpoints| endpoints.api)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| config.api_base_url.clone());
    validate_https_url(&api_base_url, "providers.copilot.api_base_url")?;

    Ok(CopilotSession {
        token: session_token,
        api_base_url,
        expires_at: token
            .expires_at
            .or_else(|| unix_timestamp().checked_add(25 * 60)),
    })
}

async fn fetch_models_with_session(
    client: &reqwest::Client,
    config: &CopilotConfig,
    session: &CopilotSession,
) -> Result<Vec<CopilotModel>> {
    let models_url =
        if config.models_url.trim().is_empty() || is_default_models_url(&config.models_url) {
            format!("{}/models", session.api_base_url.trim_end_matches('/'))
        } else {
            config.models_url.clone()
        };
    validate_https_url(&models_url, "providers.copilot.models_url")?;
    let response = client
        .get(models_url.trim())
        .headers(copilot_api_headers(config, &session.token)?)
        .timeout(request_timeout(config))
        .send()
        .await
        .context("failed to fetch GitHub Copilot models")?;
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .context("failed to read GitHub Copilot models error")?;
        bail!(
            "GitHub Copilot models returned HTTP {status}: {}",
            truncate_body(&body)
        );
    }
    let body: CopilotModelsResponse = response
        .json()
        .await
        .context("failed to parse GitHub Copilot models response")?;
    let models = body
        .data
        .into_iter()
        .chain(body.models)
        .filter(|model| model.is_selectable())
        .filter_map(|mut model| {
            model.id = model.id.trim().to_owned();
            (!model.id.is_empty()).then_some(model)
        })
        .collect::<Vec<_>>();
    if models.is_empty() {
        bail!("GitHub Copilot models response did not include model ids");
    }
    Ok(models)
}

async fn stream_sse_response(
    response: reqwest::Response,
    tx: &mpsc::Sender<AppEvent>,
) -> Result<()> {
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("failed to read GitHub Copilot response")?;
        buffer.extend_from_slice(&chunk);

        while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
            let line = buffer.drain(..=newline).collect::<Vec<_>>();
            let line = String::from_utf8_lossy(&line);
            if handle_sse_line(tx, line.trim()).await? {
                return Ok(());
            }
        }
    }

    let line = String::from_utf8_lossy(&buffer);
    if !line.trim().is_empty() && handle_sse_line(tx, line.trim()).await? {
        return Ok(());
    }
    CopilotProvider::send_event(tx, ModelEvent::Done { end_turn: true }).await;
    Ok(())
}

async fn handle_sse_line(tx: &mpsc::Sender<AppEvent>, line: &str) -> Result<bool> {
    let Some(data) = line.strip_prefix("data:") else {
        return Ok(false);
    };
    let data = data.trim();
    if data == "[DONE]" {
        CopilotProvider::send_event(tx, ModelEvent::Done { end_turn: true }).await;
        return Ok(true);
    }
    let event: serde_json::Value =
        serde_json::from_str(data).context("failed to parse GitHub Copilot stream event")?;
    for content in stream_event_text(&event) {
        CopilotProvider::send_event(tx, ModelEvent::TextDelta(content)).await;
    }
    Ok(false)
}

fn stream_event_text(event: &serde_json::Value) -> Vec<String> {
    let mut tokens = Vec::new();
    if let Some(choices) = event.get("choices").and_then(|value| value.as_array()) {
        for choice in choices {
            if let Some(content) = choice
                .get("delta")
                .and_then(|delta| delta.get("content"))
                .and_then(|content| content.as_str())
            {
                tokens.push(content.to_owned());
            }
        }
    }
    let is_delta_event = event
        .get("type")
        .and_then(|value| value.as_str())
        .is_some_and(|event_type| event_type.ends_with(".delta"));
    if is_delta_event {
        if let Some(delta) = event.get("delta").and_then(|value| value.as_str()) {
            tokens.push(delta.to_owned());
        }
    }
    if event
        .get("type")
        .and_then(|value| value.as_str())
        .is_some_and(|event_type| event_type == "content_block_delta")
    {
        if let Some(text) = event
            .get("delta")
            .and_then(|delta| delta.get("text"))
            .and_then(|text| text.as_str())
        {
            tokens.push(text.to_owned());
        }
    }
    tokens
}

fn copilot_chat_messages(request: ModelRequest) -> Vec<CopilotMessage> {
    request
        .system_prompt
        .into_iter()
        .filter(|content| !content.trim().is_empty())
        .map(|content| CopilotMessage {
            role: "system".to_owned(),
            content,
        })
        .chain(
            request
                .messages
                .into_iter()
                .filter(|message| !message.content.is_empty())
                .map(|message| CopilotMessage {
                    role: match message.role {
                        Role::User => "user".to_owned(),
                        Role::Assistant => "assistant".to_owned(),
                    },
                    content: message.content,
                }),
        )
        .collect()
}

fn copilot_conversation_messages(request: ModelRequest) -> Vec<CopilotMessage> {
    request
        .messages
        .into_iter()
        .filter(|message| !message.content.is_empty())
        .map(|message| CopilotMessage {
            role: match message.role {
                Role::User => "user".to_owned(),
                Role::Assistant => "assistant".to_owned(),
            },
            content: message.content,
        })
        .collect()
}

fn copilot_response_input(request: ModelRequest) -> Vec<CopilotResponseInput> {
    request
        .messages
        .into_iter()
        .filter(|message| !message.content.is_empty())
        .map(|message| CopilotResponseInput {
            role: match message.role {
                Role::User => "user".to_owned(),
                Role::Assistant => "assistant".to_owned(),
            },
            content: message.content,
        })
        .collect()
}

fn copilot_api_headers(config: &CopilotConfig, token: &str) -> Result<reqwest::header::HeaderMap> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::ACCEPT, "application/json".parse()?);
    headers.insert(reqwest::header::CONTENT_TYPE, "application/json".parse()?);
    headers.insert(reqwest::header::USER_AGENT, user_agent().parse()?);
    headers.insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {token}").parse()?,
    );
    headers.insert(
        "Editor-Version",
        configured_header(&config.editor_version, &user_agent()).parse()?,
    );
    headers.insert(
        "Editor-Plugin-Version",
        configured_header(&config.editor_plugin_version, &user_agent()).parse()?,
    );
    headers.insert(
        "Copilot-Integration-Id",
        configured_header(&config.integration_id, "vscode-chat").parse()?,
    );
    headers.insert("Openai-Intent", "conversation-edits".parse()?);
    headers.insert("x-initiator", "user".parse()?);
    Ok(headers)
}

fn should_use_responses_api(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    model.starts_with("gpt-5")
        || model.contains("-codex")
        || model.contains("codex-")
        || model.contains("codex")
}

fn is_default_models_url(value: &str) -> bool {
    value.trim().trim_end_matches('/') == "https://api.githubcopilot.com/models"
}

fn request_timeout(config: &CopilotConfig) -> Duration {
    Duration::from_secs(config.request_timeout_secs.max(1))
}

fn cached_copilot_session(store: Option<&AuthStore>) -> Result<Option<CopilotSession>> {
    let Some(record) = store
        .map(|store| store.record("copilot"))
        .transpose()
        .context("failed to read cached GitHub Copilot session")?
        .flatten()
    else {
        return Ok(None);
    };
    let Some(token) = record.metadata.get("copilot_session_token") else {
        return Ok(None);
    };
    if token.trim().is_empty() {
        return Ok(None);
    }
    let expires_at = record
        .metadata
        .get("copilot_session_expires_at")
        .and_then(|value| value.parse::<u64>().ok());
    if expires_at.is_some_and(|expires_at| expires_at <= unix_timestamp().saturating_add(60)) {
        return Ok(None);
    }
    let api_base_url = record
        .metadata
        .get("copilot_session_api_base_url")
        .cloned()
        .unwrap_or_default();
    if api_base_url.trim().is_empty() {
        return Ok(None);
    }
    validate_https_url(&api_base_url, "cached GitHub Copilot api base URL")?;
    Ok(Some(CopilotSession {
        token: token.clone(),
        api_base_url,
        expires_at,
    }))
}

fn save_cached_copilot_session(store: Option<&AuthStore>, session: &CopilotSession) -> Result<()> {
    let Some(store) = store else {
        return Ok(());
    };
    let Some(mut record) = store.record("copilot")? else {
        return Ok(());
    };
    record
        .metadata
        .insert("copilot_session_token".to_owned(), session.token.clone());
    record.metadata.insert(
        "copilot_session_api_base_url".to_owned(),
        session.api_base_url.clone(),
    );
    if let Some(expires_at) = session.expires_at {
        record.metadata.insert(
            "copilot_session_expires_at".to_owned(),
            expires_at.to_string(),
        );
    }
    store.upsert(record)
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn configured_header(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value.to_owned()
    }
}

fn token_candidates(
    config: &CopilotConfig,
    store: Option<&AuthStore>,
) -> Result<Vec<TokenCandidate>> {
    let mut candidates = Vec::new();
    if let Some(record) = store
        .map(|store| store.record("copilot"))
        .transpose()
        .context("failed to read GitHub Copilot credentials")?
        .flatten()
    {
        if record.status() != crate::auth::AuthStatus::Connected {
            bail!(
                "GitHub Copilot credentials are {}; run /login copilot again",
                record.status().label()
            );
        }
        if let Some(token) = record.access_token.filter(|token| !token.trim().is_empty()) {
            candidates.push(TokenCandidate {
                label: "artui auth store".to_owned(),
                token,
                cacheable: true,
            });
        }
    }

    for env_name in &config.github_token_env {
        if let Ok(token) = std::env::var(env_name) {
            if !token.trim().is_empty() {
                candidates.push(TokenCandidate {
                    label: format!("env {env_name}"),
                    token,
                    cacheable: false,
                });
            }
        }
    }

    if let Some(token) = gh_auth_token() {
        candidates.push(TokenCandidate {
            label: "gh auth token".to_owned(),
            token,
            cacheable: false,
        });
    }

    dedupe_token_candidates(candidates)
}

fn gh_auth_token() -> Option<String> {
    let output = Command::new("gh").args(["auth", "token"]).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|token| !token.is_empty())
}

fn dedupe_token_candidates(candidates: Vec<TokenCandidate>) -> Result<Vec<TokenCandidate>> {
    let mut deduped: Vec<TokenCandidate> = Vec::new();
    for candidate in candidates {
        if !deduped.iter().any(|known| known.token == candidate.token) {
            deduped.push(candidate);
        }
    }
    if deduped.is_empty() {
        bail!("GitHub Copilot is not connected; run /login copilot first");
    }
    Ok(deduped)
}

fn user_agent() -> String {
    format!("artui/{}", env!("CARGO_PKG_VERSION"))
}

fn validate_https_url(value: &str, name: &str) -> Result<()> {
    let url = reqwest::Url::parse(value.trim()).with_context(|| format!("invalid {name}"))?;
    if url.scheme() != "https" {
        bail!("{name} must use https");
    }
    if url.host_str().is_none() {
        bail!("{name} must include a host");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("{name} must not include credentials");
    }
    Ok(())
}

fn truncate_body(body: &str) -> String {
    const MAX_ERROR_BODY: usize = 600;
    let body = body.trim();
    if body.len() <= MAX_ERROR_BODY {
        body.to_owned()
    } else {
        format!("{}…", &body[..MAX_ERROR_BODY])
    }
}

#[derive(Debug)]
struct CopilotRequestError {
    status: Option<reqwest::StatusCode>,
    body: String,
}

impl CopilotRequestError {
    fn is_model_not_supported(&self) -> bool {
        self.status == Some(reqwest::StatusCode::BAD_REQUEST)
            && self.body.contains("model_not_supported")
    }

    fn is_unsupported_api_for_model(&self) -> bool {
        self.status == Some(reqwest::StatusCode::BAD_REQUEST)
            && self.body.contains("unsupported_api_for_model")
    }

    fn is_unauthorized_or_expired(&self) -> bool {
        self.status == Some(reqwest::StatusCode::UNAUTHORIZED)
            || self.body.to_ascii_lowercase().contains("expired")
    }

    fn is_session_rate_limit(&self) -> bool {
        self.status == Some(reqwest::StatusCode::TOO_MANY_REQUESTS)
            && self.body.to_ascii_lowercase().contains("5 hour session")
    }
}

impl std::fmt::Display for CopilotRequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_session_rate_limit() {
            return write!(
                formatter,
                "GitHub Copilot returned HTTP 429 rate limit from the provider. Raw response: {}",
                truncate_body(&self.body)
            );
        }
        match self.status {
            Some(status) => write!(
                formatter,
                "GitHub Copilot returned HTTP {status}: {}",
                truncate_body(&self.body)
            ),
            None => write!(
                formatter,
                "failed to connect to GitHub Copilot: {}",
                self.body
            ),
        }
    }
}

impl std::error::Error for CopilotRequestError {}

#[derive(Debug)]
struct CopilotSession {
    token: String,
    api_base_url: String,
    expires_at: Option<u64>,
}

#[derive(Debug)]
struct ResolvedCopilotSession {
    session: CopilotSession,
    api: CopilotApiKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CopilotApiKind {
    Chat,
    Responses,
    Messages,
}

struct TokenCandidate {
    label: String,
    token: String,
    cacheable: bool,
}

#[derive(Debug, Deserialize)]
struct CopilotTokenResponse {
    token: Option<String>,
    expires_at: Option<u64>,
    endpoints: Option<CopilotEndpoints>,
}

#[derive(Debug, Deserialize)]
struct CopilotEndpoints {
    api: Option<String>,
}

#[derive(Debug, Serialize)]
struct CopilotChatRequest {
    model: String,
    messages: Vec<CopilotMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct CopilotMessagesRequest {
    model: String,
    messages: Vec<CopilotMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    max_tokens: u32,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct CopilotMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct CopilotResponsesRequest {
    model: String,
    input: Vec<CopilotResponseInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<CopilotReasoning>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct CopilotReasoning {
    effort: String,
}

#[derive(Debug, Serialize)]
struct CopilotResponseInput {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct CopilotModelsResponse {
    #[serde(default)]
    data: Vec<CopilotModel>,
    #[serde(default)]
    models: Vec<CopilotModel>,
}

#[derive(Debug, Deserialize)]
struct CopilotUsageResponse {
    quota_snapshots: Option<CopilotQuotaSnapshots>,
    limited_user_quotas: Option<std::collections::BTreeMap<String, u64>>,
    monthly_quotas: Option<std::collections::BTreeMap<String, u64>>,
}

#[derive(Debug, Deserialize)]
struct CopilotQuotaSnapshots {
    premium_interactions: Option<CopilotQuotaSnapshot>,
}

#[derive(Debug, Deserialize)]
struct CopilotQuotaSnapshot {
    entitlement: Option<u64>,
    remaining: Option<u64>,
    percent_remaining: Option<f64>,
}

struct CopilotUsage {
    label: String,
}

impl CopilotUsage {
    fn from_response(response: CopilotUsageResponse) -> Option<Self> {
        if let Some(snapshot) = response
            .quota_snapshots
            .and_then(|snapshots| snapshots.premium_interactions)
        {
            let remaining = snapshot.remaining.or_else(|| {
                snapshot.entitlement.zip(snapshot.percent_remaining).map(
                    |(entitlement, percent_remaining)| {
                        ((entitlement as f64) * percent_remaining / 100.0).round() as u64
                    },
                )
            });
            if let Some(remaining) = remaining {
                let entitlement = snapshot.entitlement.unwrap_or(300);
                return Some(Self {
                    label: format!("{remaining}/{entitlement} prem"),
                });
            }
        }

        if let Some(quotas) = response.limited_user_quotas {
            let chat = quotas.get("chat").copied();
            let max_chat = response
                .monthly_quotas
                .as_ref()
                .and_then(|quotas| quotas.get("chat"))
                .copied();
            if let Some(chat) = chat {
                return Some(Self {
                    label: match max_chat {
                        Some(max_chat) => format!("{chat}/{max_chat} chat"),
                        None => format!("{chat} chat"),
                    },
                });
            }
        }
        None
    }

    fn label(self) -> String {
        self.label
    }
}

#[derive(Debug, Deserialize)]
struct CopilotModel {
    id: String,
    #[serde(default = "default_model_picker_enabled")]
    model_picker_enabled: bool,
    #[serde(default)]
    supported_endpoints: Vec<String>,
    #[serde(default)]
    policy: Option<CopilotModelPolicy>,
    #[serde(default)]
    capabilities: Option<CopilotModelCapabilities>,
}

impl CopilotModel {
    fn is_selectable(&self) -> bool {
        self.model_picker_enabled
            && self
                .policy
                .as_ref()
                .and_then(|policy| policy.state.as_deref())
                != Some("disabled")
    }

    fn api_kind(&self) -> CopilotApiKind {
        if self
            .supported_endpoints
            .iter()
            .any(|endpoint| endpoint == "/v1/messages")
        {
            CopilotApiKind::Messages
        } else if should_use_responses_api(&self.id) {
            CopilotApiKind::Responses
        } else {
            CopilotApiKind::Chat
        }
    }
}

#[derive(Debug, Deserialize)]
struct CopilotModelPolicy {
    state: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct CopilotModelCapabilities {
    #[serde(default)]
    supports: CopilotModelSupports,
    #[serde(default)]
    limits: Option<CopilotModelLimits>,
}

#[derive(Debug, Default, Deserialize)]
struct CopilotModelLimits {
    max_context_window_tokens: Option<usize>,
    max_prompt_tokens: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
struct CopilotModelSupports {
    #[serde(default)]
    reasoning_effort: Vec<String>,
}

fn default_model_picker_enabled() -> bool {
    true
}

#[derive(Debug, Serialize)]
struct CopilotModelEndpointMetadata {
    id: String,
    api: CopilotApiKind,
    supported_endpoints: Vec<String>,
    reasoning_efforts: Vec<String>,
    context_window_tokens: Option<usize>,
}

fn model_endpoint_metadata(models: &[CopilotModel]) -> Vec<CopilotModelEndpointMetadata> {
    models
        .iter()
        .map(|model| CopilotModelEndpointMetadata {
            id: model.id.clone(),
            api: model.api_kind(),
            supported_endpoints: model.supported_endpoints.clone(),
            reasoning_efforts: model
                .capabilities
                .as_ref()
                .map(|capabilities| capabilities.supports.reasoning_effort.clone())
                .unwrap_or_default(),
            context_window_tokens: model.capabilities.as_ref().and_then(|capabilities| {
                capabilities.limits.as_ref().and_then(|limits| {
                    limits
                        .max_prompt_tokens
                        .or(limits.max_context_window_tokens)
                })
            }),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn parses_stream_content_delta() {
        let (tx, mut rx) = mpsc::channel(4);

        assert!(
            !handle_sse_line(&tx, r#"data: {"choices":[{"delta":{"content":"hello"}}]}"#)
                .await
                .unwrap()
        );
        assert!(handle_sse_line(&tx, "data: [DONE]").await.unwrap());

        assert!(matches!(
            rx.recv().await.unwrap(),
            AppEvent::Model(ModelEvent::TextDelta(content)) if content == "hello"
        ));
        assert!(matches!(
            rx.recv().await.unwrap(),
            AppEvent::Model(ModelEvent::Done { .. })
        ));
    }

    #[test]
    fn extracts_responses_stream_text() {
        let event = serde_json::json!({
            "type": "response.output_text.delta",
            "delta": "hello"
        });

        assert_eq!(stream_event_text(&event), vec!["hello".to_owned()]);
    }

    #[test]
    fn extracts_messages_stream_text() {
        let event = serde_json::json!({
            "type": "content_block_delta",
            "delta": {
                "type": "text_delta",
                "text": "hello"
            }
        });

        assert_eq!(stream_event_text(&event), vec!["hello".to_owned()]);
    }

    #[test]
    fn ignores_responses_final_aggregate_text() {
        let event = serde_json::json!({
            "type": "response.completed",
            "output": [
                {
                    "content": [
                        {"type": "output_text", "text": "hello"}
                    ]
                }
            ]
        });

        assert!(stream_event_text(&event).is_empty());
    }

    #[test]
    fn detects_unsupported_api_error() {
        let error = CopilotRequestError {
            status: Some(reqwest::StatusCode::BAD_REQUEST),
            body: r#"{"error":{"code":"unsupported_api_for_model"}}"#.to_owned(),
        };

        assert!(error.is_unsupported_api_for_model());
    }

    #[test]
    fn explains_copilot_session_rate_limit() {
        let error = CopilotRequestError {
            status: Some(reqwest::StatusCode::TOO_MANY_REQUESTS),
            body: "Sorry, you've exceeded your 5 hour session limits.".to_owned(),
        };

        assert!(error.is_session_rate_limit());
        assert!(error.to_string().contains("HTTP 429 rate limit"));
    }

    #[test]
    fn routes_copilot_codex_models_to_responses() {
        assert!(should_use_responses_api("gpt-5.4-mini"));
        assert!(should_use_responses_api("gpt-5.2-codex"));
        assert!(should_use_responses_api("codex-mini"));
        assert!(!should_use_responses_api("gpt-4.1"));
    }

    #[test]
    fn messages_endpoint_takes_precedence_over_name_heuristics() {
        let model = CopilotModel {
            id: "claude-sonnet-4.6".to_owned(),
            model_picker_enabled: true,
            supported_endpoints: vec!["/v1/messages".to_owned()],
            policy: None,
            capabilities: None,
        };

        assert_eq!(model.api_kind(), CopilotApiKind::Messages);
    }

    #[test]
    fn filters_disabled_copilot_picker_models() {
        assert!(CopilotModel {
            id: "enabled".to_owned(),
            model_picker_enabled: true,
            supported_endpoints: Vec::new(),
            policy: None,
            capabilities: None,
        }
        .is_selectable());
        assert!(!CopilotModel {
            id: "hidden".to_owned(),
            model_picker_enabled: false,
            supported_endpoints: Vec::new(),
            policy: None,
            capabilities: None,
        }
        .is_selectable());
        assert!(!CopilotModel {
            id: "disabled".to_owned(),
            model_picker_enabled: true,
            supported_endpoints: Vec::new(),
            policy: Some(CopilotModelPolicy {
                state: Some("disabled".to_owned()),
            }),
            capabilities: None,
        }
        .is_selectable());
    }

    #[test]
    fn rejects_non_https_copilot_urls() {
        assert!(validate_https_url("http://example.com", "test").is_err());
        assert!(validate_https_url("https://user@example.com", "test").is_err());
        assert!(validate_https_url("https://example.com", "test").is_ok());
    }
}
