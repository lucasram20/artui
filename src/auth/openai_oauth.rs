//! OpenAI ChatGPT subscription OAuth 2.0 PKCE login.
//!
//! Implements the same authorization-code + PKCE flow used by the OpenAI
//! Codex CLI. The user's ChatGPT subscription tokens are persisted in the
//! shared `AuthStore` under provider id `openai_account`.
//!
//! Endpoints (issuer `https://auth.openai.com`):
//!   - `GET  /oauth/authorize` — opens in browser
//!   - `POST /oauth/token`     — code -> tokens, refresh_token -> tokens
//!
//! Loopback redirect: `http://localhost:1455/auth/callback` (fallback 1457).

use std::{
    collections::BTreeMap,
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
    time::timeout,
};

use crate::{
    app::{AppEvent, AuthEvent},
    auth::{AuthRecord, AuthStore},
};

/// Codex CLI's public OAuth client_id (see openai/codex codex-rs/login).
pub const DEFAULT_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const DEFAULT_ISSUER: &str = "https://auth.openai.com";
pub const DEFAULT_PORT: u16 = 1455;
pub const FALLBACK_PORT: u16 = 1457;
pub const DEFAULT_SCOPE: &str =
    "openid profile email offline_access api.connectors.read api.connectors.invoke";
pub const DEFAULT_TIMEOUT_SECS: u64 = 300;
pub const DEFAULT_PROVIDER_ID: &str = "openai_account";

#[derive(Debug, Clone)]
pub struct OpenAiOAuthConfig {
    pub provider_id: String,
    pub client_id: String,
    pub issuer: String,
    pub scope: String,
    pub port: u16,
    pub fallback_port: u16,
    pub timeout_secs: u64,
}

impl Default for OpenAiOAuthConfig {
    fn default() -> Self {
        Self {
            provider_id: DEFAULT_PROVIDER_ID.to_owned(),
            client_id: DEFAULT_CLIENT_ID.to_owned(),
            issuer: DEFAULT_ISSUER.to_owned(),
            scope: DEFAULT_SCOPE.to_owned(),
            port: DEFAULT_PORT,
            fallback_port: FALLBACK_PORT,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }
}

/// Public entrypoint — runs the full PKCE flow and persists tokens.
pub async fn run_openai_oauth_login(
    config: OpenAiOAuthConfig,
    store: AuthStore,
    tx: mpsc::Sender<AppEvent>,
) {
    if let Err(error) = openai_oauth_login(config, store, &tx).await {
        let message = format!("OpenAI login failed: {error}");
        send_status(&tx, message.clone()).await;
        send_message(&tx, message).await;
    }
}

async fn openai_oauth_login(
    config: OpenAiOAuthConfig,
    store: AuthStore,
    tx: &mpsc::Sender<AppEvent>,
) -> Result<()> {
    validate_config(&config)?;

    // 1. Bind loopback listener (try preferred port, then fallback).
    let (listener, port) = bind_loopback(config.port, config.fallback_port).await?;
    let redirect_uri = format!("http://localhost:{port}/auth/callback");

    // 2. Generate PKCE + state.
    let pkce = generate_pkce();
    let state = generate_state();

    // 3. Build authorize URL and open browser.
    let auth_url = build_authorize_url(
        &config.issuer,
        &config.client_id,
        &redirect_uri,
        &pkce.code_challenge,
        &state,
        &config.scope,
    );

    let browser_status = open_browser(&auth_url)
        .map(|()| "Opened the OpenAI sign-in page in your browser.".to_owned())
        .unwrap_or_else(|error| format!("Could not open browser automatically: {error}"));

    send_message(
        tx,
        format!(
            "OpenAI ChatGPT login:\n{browser_status}\nIf nothing opens, visit:\n  {auth_url}\nWaiting for callback on {redirect_uri}"
        ),
    )
    .await;
    send_status(tx, "Waiting for OpenAI authorization".to_owned()).await;

    // 4. Wait for the loopback callback (with timeout).
    let wait = Duration::from_secs(config.timeout_secs.max(1));
    let callback = match timeout(wait, wait_for_callback(listener)).await {
        Ok(Ok(callback)) => callback,
        Ok(Err(error)) => return Err(error),
        Err(_) => bail!(
            "OpenAI login timed out after {} seconds",
            config.timeout_secs
        ),
    };

    if callback.state != state {
        bail!("OAuth state mismatch — login aborted (possible CSRF)");
    }

    send_status(tx, "Exchanging authorization code".to_owned()).await;

    // 5. Exchange code for tokens.
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("failed to build OpenAI OAuth HTTP client")?;
    let tokens = exchange_code(
        &http,
        &config.issuer,
        &config.client_id,
        &redirect_uri,
        &callback.code,
        &pkce.code_verifier,
    )
    .await?;

    // 6. Parse id_token JWT (best-effort; persist raw if parse fails).
    let id_claims = parse_jwt_claims(&tokens.id_token).unwrap_or_default();
    let account_id = id_claims
        .chatgpt_account_id
        .clone()
        .or_else(|| id_claims.account_id.clone());
    let plan_type = id_claims.chatgpt_plan_type.clone();
    let email = id_claims.email.clone();

    // 7. Persist to AuthStore.
    let now = unix_timestamp();
    let expires_at = tokens.expires_in.and_then(|s| now.checked_add(s));

    let mut metadata = BTreeMap::new();
    metadata.insert("source".to_owned(), "openai-oauth-pkce".to_owned());
    metadata.insert("issuer".to_owned(), config.issuer.clone());
    if let Some(id_token) = tokens.id_token.split_whitespace().next() {
        if !id_token.is_empty() {
            metadata.insert("id_token".to_owned(), tokens.id_token.clone());
        }
    }
    if let Some(plan) = plan_type.filter(|v| !v.is_empty()) {
        metadata.insert("plan_type".to_owned(), plan);
    }
    if let Some(token_type) = tokens.token_type.filter(|v| !v.is_empty()) {
        metadata.insert("token_type".to_owned(), token_type);
    }
    if let Some(scope) = tokens.scope.filter(|v| !v.is_empty()) {
        metadata.insert("scope".to_owned(), scope);
    }
    if let Some(account_id) = &account_id {
        metadata.insert("chatgpt_account_id".to_owned(), account_id.clone());
    }

    let account_label = email
        .clone()
        .or_else(|| account_id.clone())
        .or_else(|| Some("openai-oauth-pkce".to_owned()));

    store.upsert(AuthRecord {
        provider_id: config.provider_id.clone(),
        account_label,
        access_token: Some(tokens.access_token),
        refresh_token: tokens.refresh_token,
        expires_at,
        updated_at: 0,
        metadata,
    })?;

    send_status(tx, "OpenAI login complete".to_owned()).await;
    send_message(
        tx,
        match email {
            Some(email) => format!("OpenAI ChatGPT credentials saved locally for {email}."),
            None => "OpenAI ChatGPT credentials saved locally.".to_owned(),
        },
    )
    .await;
    Ok(())
}

// ---------------------------------------------------------------------------
// PKCE + state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct PkceCodes {
    pub code_verifier: String,
    pub code_challenge: String,
}

pub(crate) fn generate_pkce() -> PkceCodes {
    let mut bytes = [0u8; 64];
    rand::thread_rng().fill_bytes(&mut bytes);
    // Verifier: URL-safe base64 without padding (43..128 chars).
    let code_verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    // Challenge (S256): BASE64URL-ENCODE(SHA256(verifier)) without padding.
    let digest = Sha256::digest(code_verifier.as_bytes());
    let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    PkceCodes {
        code_verifier,
        code_challenge,
    }
}

pub(crate) fn generate_state() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub(crate) fn build_authorize_url(
    issuer: &str,
    client_id: &str,
    redirect_uri: &str,
    code_challenge: &str,
    state: &str,
    scope: &str,
) -> String {
    let issuer = issuer.trim_end_matches('/');
    let pairs: Vec<(&str, &str)> = vec![
        ("response_type", "code"),
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("scope", scope),
        ("code_challenge", code_challenge),
        ("code_challenge_method", "S256"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("state", state),
    ];
    let qs = pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={}", urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{issuer}/oauth/authorize?{qs}")
}

// ---------------------------------------------------------------------------
// Loopback HTTP server
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct CallbackParams {
    pub code: String,
    pub state: String,
}

async fn bind_loopback(port: u16, fallback: u16) -> Result<(TcpListener, u16)> {
    match TcpListener::bind(("127.0.0.1", port)).await {
        Ok(listener) => Ok((listener, port)),
        Err(_primary) => {
            let listener = TcpListener::bind(("127.0.0.1", fallback))
                .await
                .with_context(|| {
                    format!(
                        "failed to bind loopback callback on port {port} or fallback {fallback}"
                    )
                })?;
            Ok((listener, fallback))
        }
    }
}

/// Wait for the OAuth callback. Replies with a small success/error HTML page
/// to whichever browser tab triggered the redirect, then returns the parsed
/// query parameters.
async fn wait_for_callback(listener: TcpListener) -> Result<CallbackParams> {
    loop {
        let (mut socket, _) = listener
            .accept()
            .await
            .context("loopback callback accept failed")?;

        // Read just enough to parse the request line + headers.
        let mut buf = vec![0u8; 8192];
        let mut total = 0usize;
        loop {
            let n = socket
                .read(&mut buf[total..])
                .await
                .context("loopback callback read failed")?;
            if n == 0 {
                break;
            }
            total += n;
            // Bail once headers terminate or buffer fills.
            if buf[..total].windows(4).any(|w| w == b"\r\n\r\n") || total == buf.len() {
                break;
            }
        }
        let request = String::from_utf8_lossy(&buf[..total]).to_string();
        let request_line = request.lines().next().unwrap_or("").to_owned();

        // Only accept GET /auth/callback...
        let path = parse_request_path(&request_line);
        let Some(query) = path
            .strip_prefix("/auth/callback")
            .and_then(|rest| rest.strip_prefix('?'))
        else {
            // Reply 404 to anything else (favicon, probes), keep listening.
            let _ = write_http_response(
                &mut socket,
                404,
                "Not Found",
                "text/plain; charset=utf-8",
                "404 Not Found",
            )
            .await;
            continue;
        };

        let params = parse_query(query);
        if let Some(error) = params.get("error") {
            let description = params
                .get("error_description")
                .cloned()
                .unwrap_or_else(|| error.clone());
            let html = error_html(&description);
            let _ = write_http_response(
                &mut socket,
                400,
                "Bad Request",
                "text/html; charset=utf-8",
                &html,
            )
            .await;
            bail!("authorization server returned error: {description}");
        }

        let code = params
            .get("code")
            .cloned()
            .ok_or_else(|| anyhow!("callback missing 'code' parameter"))?;
        let state = params
            .get("state")
            .cloned()
            .ok_or_else(|| anyhow!("callback missing 'state' parameter"))?;

        let _ = write_http_response(
            &mut socket,
            200,
            "OK",
            "text/html; charset=utf-8",
            SUCCESS_HTML,
        )
        .await;
        return Ok(CallbackParams { code, state });
    }
}

fn parse_request_path(request_line: &str) -> &str {
    // "GET /auth/callback?... HTTP/1.1"
    let mut parts = request_line.split_whitespace();
    let _method = parts.next().unwrap_or("");
    parts.next().unwrap_or("")
}

fn parse_query(qs: &str) -> std::collections::HashMap<String, String> {
    qs.split('&')
        .filter_map(|kv| {
            let mut it = kv.splitn(2, '=');
            let k = it.next()?;
            let v = it.next().unwrap_or("");
            let k = urlencoding::decode(k).ok()?.into_owned();
            let v = urlencoding::decode(v).ok().map(|c| c.into_owned());
            Some((k, v.unwrap_or_default()))
        })
        .collect()
}

async fn write_http_response(
    socket: &mut tokio::net::TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &str,
) -> Result<()> {
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{body}",
        len = body.len()
    );
    socket
        .write_all(response.as_bytes())
        .await
        .context("loopback callback write failed")?;
    let _ = socket.shutdown().await;
    Ok(())
}

const SUCCESS_HTML: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>artui — login complete</title><style>body{font-family:system-ui,sans-serif;background:#0b0d12;color:#cbd5e1;display:flex;align-items:center;justify-content:center;height:100vh;margin:0}div{text-align:center;max-width:480px;padding:32px;border:1px solid #1f2937;border-radius:12px;background:#111827}h1{color:#22d3ee;margin:0 0 8px;font-size:20px}p{margin:8px 0;color:#94a3b8;font-size:14px}</style></head><body><div><h1>Sign-in complete</h1><p>You can close this tab and return to your terminal.</p></div></body></html>";

fn error_html(description: &str) -> String {
    let safe: String = description
        .chars()
        .map(|c| match c {
            '<' => "&lt;".to_owned(),
            '>' => "&gt;".to_owned(),
            '&' => "&amp;".to_owned(),
            '"' => "&quot;".to_owned(),
            other => other.to_string(),
        })
        .collect();
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>artui — login error</title></head><body style=\"font-family:system-ui,sans-serif;background:#0b0d12;color:#cbd5e1;padding:32px\"><h1 style=\"color:#f87171\">Sign-in failed</h1><p>{safe}</p><p>You can close this tab and try again from your terminal.</p></body></html>"
    )
}

// ---------------------------------------------------------------------------
// Token exchange + refresh
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub id_token: String,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub scope: Option<String>,
}

async fn exchange_code(
    client: &reqwest::Client,
    issuer: &str,
    client_id: &str,
    redirect_uri: &str,
    code: &str,
    code_verifier: &str,
) -> Result<TokenResponse> {
    let token_endpoint = format!("{}/oauth/token", issuer.trim_end_matches('/'));
    let body = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
        urlencoding::encode(code),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(client_id),
        urlencoding::encode(code_verifier)
    );
    let resp = client
        .post(&token_endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .context("OpenAI token exchange request failed")?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .context("failed to read OpenAI token response")?;
    if !status.is_success() {
        bail!("OpenAI token exchange returned HTTP {status}");
    }
    serde_json::from_str(&text).context("failed to parse OpenAI token response")
}

/// Refresh an existing access token using the stored refresh token.
/// Exposed so the streaming provider can recover from `401`.
pub async fn refresh_access_token(
    client: &reqwest::Client,
    issuer: &str,
    client_id: &str,
    refresh_token: &str,
    scope: &str,
) -> Result<TokenResponse> {
    let token_endpoint = format!("{}/oauth/token", issuer.trim_end_matches('/'));
    let body = format!(
        "grant_type=refresh_token&refresh_token={}&client_id={}&scope={}",
        urlencoding::encode(refresh_token),
        urlencoding::encode(client_id),
        urlencoding::encode(scope)
    );
    let resp = client
        .post(&token_endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .context("OpenAI refresh request failed")?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .context("failed to read OpenAI refresh response")?;
    if !status.is_success() {
        bail!("OpenAI refresh returned HTTP {status}");
    }
    serde_json::from_str(&text).context("failed to parse OpenAI refresh response")
}

// ---------------------------------------------------------------------------
// JWT id_token claim parsing (no signature verification — issuer is trusted
// because the token came back over TLS from the configured token endpoint).
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
pub(crate) struct IdTokenClaims {
    pub email: Option<String>,
    pub chatgpt_account_id: Option<String>,
    pub chatgpt_plan_type: Option<String>,
    pub account_id: Option<String>,
}

pub(crate) fn parse_jwt_claims(jwt: &str) -> Option<IdTokenClaims> {
    let mut parts = jwt.split('.');
    let _header = parts.next()?;
    let payload_b64 = parts.next()?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload_b64))
        .ok()?;
    let payload: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let auth = payload.get("https://api.openai.com/auth");
    Some(IdTokenClaims {
        email: payload
            .get("email")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
        chatgpt_account_id: auth
            .and_then(|a| a.get("chatgpt_account_id"))
            .and_then(|v| v.as_str())
            .map(str::to_owned),
        chatgpt_plan_type: auth
            .and_then(|a| a.get("chatgpt_plan_type"))
            .and_then(|v| v.as_str())
            .map(str::to_owned),
        account_id: auth
            .and_then(|a| a.get("account_id"))
            .and_then(|v| v.as_str())
            .map(str::to_owned),
    })
}

// ---------------------------------------------------------------------------
// Misc helpers
// ---------------------------------------------------------------------------

fn validate_config(config: &OpenAiOAuthConfig) -> Result<()> {
    if config.client_id.trim().is_empty() {
        bail!("missing providers.openai_account.oauth_client_id");
    }
    let url = reqwest::Url::parse(config.issuer.trim())
        .context("invalid providers.openai_account.issuer")?;
    if url.scheme() != "https" {
        bail!("providers.openai_account.issuer must use https");
    }
    if url.host_str().is_none() {
        bail!("providers.openai_account.issuer must include a host");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("providers.openai_account.issuer must not include credentials");
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
    );
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
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_verifier_and_challenge_are_distinct_and_url_safe() {
        let p = generate_pkce();
        assert!(!p.code_verifier.is_empty());
        assert!(!p.code_challenge.is_empty());
        assert_ne!(p.code_verifier, p.code_challenge);
        // Verifier length: 64 bytes -> 86 base64url chars (no padding).
        assert!(p.code_verifier.len() >= 43 && p.code_verifier.len() <= 128);
        // No '+', '/', '=' (url-safe, unpadded).
        for ch in p.code_verifier.chars() {
            assert!(
                ch.is_ascii_alphanumeric() || ch == '-' || ch == '_',
                "verifier has non-url-safe char: {ch}"
            );
        }
        for ch in p.code_challenge.chars() {
            assert!(
                ch.is_ascii_alphanumeric() || ch == '-' || ch == '_',
                "challenge has non-url-safe char: {ch}"
            );
        }
    }

    #[test]
    fn pkce_challenge_is_sha256_of_verifier() {
        let p = generate_pkce();
        let digest = Sha256::digest(p.code_verifier.as_bytes());
        let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
        assert_eq!(p.code_challenge, expected);
    }

    #[test]
    fn state_is_random_url_safe() {
        let a = generate_state();
        let b = generate_state();
        assert_ne!(a, b);
        for ch in a.chars() {
            assert!(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
        }
    }

    #[test]
    fn authorize_url_contains_pkce_and_scope() {
        let url = build_authorize_url(
            "https://auth.openai.com",
            "app_test",
            "http://localhost:1455/auth/callback",
            "CHALLENGE",
            "STATE",
            "openid profile",
        );
        assert!(url.starts_with("https://auth.openai.com/oauth/authorize?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=app_test"));
        assert!(url.contains("code_challenge=CHALLENGE"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=STATE"));
        // Scope spaces must be URL-encoded.
        assert!(url.contains("scope=openid%20profile"));
        // redirect_uri must be encoded.
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback"));
    }

    #[test]
    fn authorize_url_strips_trailing_slash_on_issuer() {
        let url = build_authorize_url(
            "https://auth.openai.com/",
            "x",
            "http://localhost:1/auth/callback",
            "c",
            "s",
            "openid",
        );
        assert!(url.starts_with("https://auth.openai.com/oauth/authorize?"));
    }

    #[test]
    fn parse_query_handles_url_encoded_values() {
        let q = parse_query("code=abc%20def&state=xyz");
        assert_eq!(q.get("code").map(String::as_str), Some("abc def"));
        assert_eq!(q.get("state").map(String::as_str), Some("xyz"));
    }

    #[test]
    fn parse_jwt_claims_decodes_chatgpt_fields() {
        let payload = serde_json::json!({
            "email": "alice@example.com",
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "acct_abc",
                "chatgpt_plan_type": "plus",
                "account_id": "acct_abc",
            }
        });
        let payload_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&payload).unwrap());
        let jwt = format!("HEADER.{payload_b64}.SIG");
        let claims = parse_jwt_claims(&jwt).expect("claims should parse");
        assert_eq!(claims.email.as_deref(), Some("alice@example.com"));
        assert_eq!(claims.chatgpt_account_id.as_deref(), Some("acct_abc"));
        assert_eq!(claims.chatgpt_plan_type.as_deref(), Some("plus"));
    }

    #[test]
    fn parse_jwt_claims_returns_none_for_garbage() {
        assert!(parse_jwt_claims("not-a-jwt").is_none());
        assert!(parse_jwt_claims("only.two").is_none());
    }

    #[test]
    fn validate_config_rejects_non_https_issuer() {
        let cfg = OpenAiOAuthConfig {
            issuer: "http://auth.openai.com".to_owned(),
            ..OpenAiOAuthConfig::default()
        };
        assert!(validate_config(&cfg).is_err());
    }

    #[test]
    fn validate_config_rejects_empty_client_id() {
        let cfg = OpenAiOAuthConfig {
            client_id: " ".to_owned(),
            ..OpenAiOAuthConfig::default()
        };
        assert!(validate_config(&cfg).is_err());
    }

    #[test]
    fn validate_config_accepts_default() {
        assert!(validate_config(&OpenAiOAuthConfig::default()).is_ok());
    }

    #[test]
    fn parse_request_path_extracts_path_with_query() {
        assert_eq!(
            parse_request_path("GET /auth/callback?code=abc&state=xyz HTTP/1.1"),
            "/auth/callback?code=abc&state=xyz"
        );
        assert_eq!(parse_request_path("GET / HTTP/1.1"), "/");
        assert_eq!(parse_request_path(""), "");
    }

    #[test]
    fn error_html_escapes_special_chars() {
        let html = error_html("<script>alert(1)</script>");
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }
}
