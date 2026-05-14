use std::{
    collections::BTreeMap,
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use tokio::{sync::mpsc, time::sleep};

use crate::{
    app::{AppEvent, AuthEvent},
    auth::{AuthRecord, AuthStore},
    config::CopilotConfig,
    providers::copilot::fetch_copilot_models,
};

const DEVICE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";

#[derive(Debug, Clone)]
pub struct GitHubDeviceFlowConfig {
    pub provider_id: String,
    pub client_id: String,
    pub device_code_url: String,
    pub token_url: String,
    pub scope: String,
    pub timeout_secs: u64,
}

pub async fn run_github_device_login(
    config: GitHubDeviceFlowConfig,
    copilot_config: CopilotConfig,
    store: AuthStore,
    tx: mpsc::Sender<AppEvent>,
) {
    if let Err(error) = github_device_login(config, copilot_config, store, &tx).await {
        let message = format!("Login failed: {error}");
        send_status(&tx, message.clone()).await;
        send_message(&tx, message).await;
    }
}

async fn github_device_login(
    config: GitHubDeviceFlowConfig,
    copilot_config: CopilotConfig,
    store: AuthStore,
    tx: &mpsc::Sender<AppEvent>,
) -> Result<()> {
    validate_config(&config)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("failed to build GitHub OAuth HTTP client")?;
    let device = request_device_code(&client, &config).await?;
    let browser_status = open_browser(device.verification_uri.as_str())
        .map(|()| "Opened the GitHub login page in your browser.".to_owned())
        .unwrap_or_else(|error| format!("Could not open browser automatically: {error}"));
    send_message(
        tx,
        format!(
            "GitHub device login:\n{browser_status}\nOpen {}\nEnter code {}\nThis code expires in {} seconds.",
            device.verification_uri, device.user_code, device.expires_in
        ),
    )
    .await;
    send_status(tx, "Waiting for GitHub authorization".to_owned()).await;

    let token = poll_for_token(&client, &config, &device, tx).await?;
    let expires_at = token
        .expires_in
        .and_then(|seconds| unix_timestamp().checked_add(seconds));
    let refresh_expires_at = token
        .refresh_token_expires_in
        .and_then(|seconds| unix_timestamp().checked_add(seconds));
    let mut metadata = BTreeMap::new();
    metadata.insert("source".to_owned(), "github-device-flow".to_owned());
    if let Some(token_type) = token.token_type.filter(|value| !value.is_empty()) {
        metadata.insert("token_type".to_owned(), token_type);
    }
    if let Some(scope) = token.scope.filter(|value| !value.is_empty()) {
        metadata.insert("scope".to_owned(), scope);
    }
    if let Some(refresh_expires_at) = refresh_expires_at {
        metadata.insert(
            "refresh_expires_at".to_owned(),
            refresh_expires_at.to_string(),
        );
    }

    let provider_id = config.provider_id;
    store.upsert(AuthRecord {
        provider_id: provider_id.clone(),
        account_label: Some("github-device-flow".to_owned()),
        access_token: Some(token.access_token),
        refresh_token: token.refresh_token,
        expires_at,
        updated_at: 0,
        metadata,
    })?;

    match fetch_copilot_models(&copilot_config, &store).await {
        Ok(models) => {
            let mut record = store
                .record(&provider_id)?
                .context("GitHub Copilot credentials disappeared after login")?;
            record
                .metadata
                .insert("models".to_owned(), serde_json::to_string(&models)?);
            store.upsert(record)?;
            let _ = tx
                .send(AppEvent::Auth(AuthEvent::CopilotModels(Ok(models))))
                .await;
        }
        Err(error) => {
            send_message(
                tx,
                format!("GitHub Copilot login succeeded, but model discovery failed: {error}"),
            )
            .await;
        }
    }

    send_status(tx, "GitHub Copilot login complete".to_owned()).await;
    send_message(tx, "GitHub Copilot credentials saved locally.".to_owned()).await;
    Ok(())
}

fn validate_config(config: &GitHubDeviceFlowConfig) -> Result<()> {
    if config.client_id.trim().is_empty() {
        bail!("missing providers.copilot.github_oauth_client_id");
    }
    if config.device_code_url.trim().is_empty() {
        bail!("missing providers.copilot.github_device_code_url");
    }
    if config.token_url.trim().is_empty() {
        bail!("missing providers.copilot.github_oauth_token_url");
    }
    validate_oauth_url(
        &config.device_code_url,
        "providers.copilot.github_device_code_url",
    )?;
    validate_oauth_url(
        &config.token_url,
        "providers.copilot.github_oauth_token_url",
    )?;
    Ok(())
}

fn validate_oauth_url(value: &str, name: &str) -> Result<()> {
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

fn open_browser(url: &str) -> Result<()> {
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "windows") {
        &[("rundll32", &["url.dll,FileProtocolHandler", url])]
    } else if cfg!(target_os = "macos") {
        &[("open", &[url])]
    } else {
        &[
            ("xdg-open", &[url]),
            ("gio", &["open", url]),
            ("gnome-open", &[url]),
            ("kde-open", &[url]),
        ]
    };

    let mut last_error = None;
    for (program, args) in candidates {
        match Command::new(program).args(*args).spawn() {
            Ok(_) => return Ok(()),
            Err(error) => last_error = Some(format!("{program}: {error}")),
        }
    }

    bail!(
        "{}",
        last_error.unwrap_or_else(|| "no browser opener command is available".to_owned())
    )
}

async fn request_device_code(
    client: &reqwest::Client,
    config: &GitHubDeviceFlowConfig,
) -> Result<DeviceCodeResponse> {
    let mut form = vec![("client_id", config.client_id.as_str())];
    if !config.scope.trim().is_empty() {
        form.push(("scope", config.scope.as_str()));
    }

    let response = client
        .post(config.device_code_url.trim())
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&form)
        .send()
        .await
        .context("failed to request GitHub device code")?;
    let status = response.status();
    let body = response
        .text()
        .await
        .context("failed to read GitHub device code response")?;
    if !status.is_success() {
        bail!("GitHub device code request returned HTTP {status}");
    }

    serde_json::from_str(&body).context("failed to parse GitHub device code response")
}

async fn poll_for_token(
    client: &reqwest::Client,
    config: &GitHubDeviceFlowConfig,
    device: &DeviceCodeResponse,
    tx: &mpsc::Sender<AppEvent>,
) -> Result<TokenSuccessResponse> {
    let timeout_secs = config.timeout_secs.min(device.expires_in).max(1);
    let deadline = unix_timestamp().saturating_add(timeout_secs);
    let mut interval = device.interval.unwrap_or(5).max(1);

    loop {
        if unix_timestamp() >= deadline {
            bail!("GitHub device login timed out");
        }

        sleep(Duration::from_secs(interval)).await;
        let response = client
            .post(config.token_url.trim())
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&[
                ("client_id", config.client_id.as_str()),
                ("device_code", device.device_code.as_str()),
                ("grant_type", DEVICE_GRANT_TYPE),
            ])
            .send()
            .await
            .context("failed to poll GitHub token endpoint")?;
        let status = response.status();
        let body = response
            .text()
            .await
            .context("failed to read GitHub token response")?;
        if !status.is_success() {
            bail!("GitHub token endpoint returned HTTP {status}");
        }

        let token: TokenResponse =
            serde_json::from_str(&body).context("failed to parse GitHub token response")?;
        match token {
            TokenResponse::Success(success) => return Ok(success),
            TokenResponse::Pending { interval: next } => {
                if let Some(next) = next {
                    interval = next.max(1);
                }
                send_status(tx, "Waiting for GitHub authorization".to_owned()).await;
            }
            TokenResponse::SlowDown { interval: next } => {
                interval = next.unwrap_or(interval.saturating_add(5)).max(1);
                send_status(tx, "GitHub asked artui to slow polling".to_owned()).await;
            }
            TokenResponse::AccessDenied { description } => {
                bail!(description.unwrap_or_else(|| "GitHub device login denied".to_owned()));
            }
            TokenResponse::Expired { description } => {
                bail!(description.unwrap_or_else(|| "GitHub device code expired".to_owned()));
            }
            TokenResponse::Error { error, description } => {
                bail!(description.unwrap_or(error));
            }
        }
    }
}

async fn send_status(tx: &mpsc::Sender<AppEvent>, status: String) {
    let _ = tx.send(AppEvent::Auth(AuthEvent::Status(status))).await;
}

async fn send_message(tx: &mpsc::Sender<AppEvent>, message: String) {
    let _ = tx.send(AppEvent::Auth(AuthEvent::Message(message))).await;
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: Option<u64>,
}

#[derive(Debug)]
enum TokenResponse {
    Success(TokenSuccessResponse),
    Pending {
        interval: Option<u64>,
    },
    SlowDown {
        interval: Option<u64>,
    },
    AccessDenied {
        description: Option<String>,
    },
    Expired {
        description: Option<String>,
    },
    Error {
        error: String,
        description: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct TokenSuccessResponse {
    access_token: String,
    token_type: Option<String>,
    scope: Option<String>,
    expires_in: Option<u64>,
    refresh_token: Option<String>,
    refresh_token_expires_in: Option<u64>,
}

impl<'de> Deserialize<'de> for TokenResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawTokenResponse {
            Success(TokenSuccessResponse),
            Error {
                error: String,
                error_description: Option<String>,
                interval: Option<u64>,
            },
        }

        let response = match RawTokenResponse::deserialize(deserializer)? {
            RawTokenResponse::Success(success) => TokenResponse::Success(success),
            RawTokenResponse::Error {
                error,
                error_description,
                interval,
            } => {
                let description =
                    error_description.filter(|description| !description.trim().is_empty());
                match error.as_str() {
                    "authorization_pending" => TokenResponse::Pending { interval },
                    "slow_down" => TokenResponse::SlowDown { interval },
                    "access_denied" => TokenResponse::AccessDenied { description },
                    "expired_token" => TokenResponse::Expired { description },
                    _ => TokenResponse::Error { error, description },
                }
            }
        };
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_success_token_response() {
        let response: TokenResponse = serde_json::from_str(
            r#"{"access_token":"secret","token_type":"bearer","scope":"read:user","expires_in":3600}"#,
        )
        .unwrap();

        match response {
            TokenResponse::Success(success) => {
                assert_eq!(success.access_token, "secret");
                assert_eq!(success.expires_in, Some(3600));
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[test]
    fn parses_pending_token_response() {
        let response: TokenResponse =
            serde_json::from_str(r#"{"error":"authorization_pending","interval":7}"#).unwrap();

        match response {
            TokenResponse::Pending { interval } => assert_eq!(interval, Some(7)),
            other => panic!("expected pending, got {other:?}"),
        }
    }

    #[test]
    fn parses_slow_down_token_response() {
        let response: TokenResponse =
            serde_json::from_str(r#"{"error":"slow_down","interval":10}"#).unwrap();

        match response {
            TokenResponse::SlowDown { interval } => assert_eq!(interval, Some(10)),
            other => panic!("expected slow_down, got {other:?}"),
        }
    }

    #[test]
    fn parses_denied_token_response() {
        let response: TokenResponse = serde_json::from_str(
            r#"{"error":"access_denied","error_description":"authorization denied"}"#,
        )
        .unwrap();

        match response {
            TokenResponse::AccessDenied { description } => {
                assert_eq!(description.as_deref(), Some("authorization denied"));
            }
            other => panic!("expected access_denied, got {other:?}"),
        }
    }
}
