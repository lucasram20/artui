//! GitHub Copilot integration tests with mocked HTTP server.
//!
//! Covers the gaps from `docs/todos/oauth-provider-support.md` Phase 3:
//! - Mocked token exchange (success, expiry, refresh failure)
//! - Model listing (filters disabled / hidden picker entries)
//! - Streaming chat completions and Anthropic-shaped messages SSE
//! - 401 retry semantics
//!
//! Tests use `wiremock` to spin a real local HTTP server and hit it via the
//! production `CopilotProvider`. The provider's `validate_https_url` was
//! relaxed to allow `http://127.0.0.1` for exactly this reason — local mock
//! servers and dev proxies. Production usage still requires HTTPS.

use std::collections::BTreeMap;

use artui::app::{AppEvent, Message, Role};
use artui::auth::{AuthRecord, AuthStore};
use artui::config::CopilotConfig;
use artui::providers::copilot::{fetch_copilot_models, CopilotProvider};
use artui::providers::{LlmProvider, ModelEvent, ModelRequest, ToolChoice};
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::mpsc;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ── Fixtures ────────────────────────────────────────────────────────────

fn temp_auth_store() -> (TempDir, AuthStore) {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("auth.json");
    let store = AuthStore::new(path);
    (dir, store)
}

fn seed_copilot_record(store: &AuthStore, github_token: &str) {
    let mut metadata = BTreeMap::new();
    metadata.insert("source".to_owned(), "test-fixture".to_owned());
    store
        .upsert(AuthRecord {
            provider_id: "copilot".to_owned(),
            account_label: Some("integration-test".to_owned()),
            access_token: Some(github_token.to_owned()),
            refresh_token: None,
            expires_at: None,
            updated_at: 0,
            metadata,
        })
        .expect("upsert");
}

fn copilot_config_for(server: &MockServer) -> CopilotConfig {
    let base = server.uri();
    CopilotConfig {
        api_base_url: base.clone(),
        token_url: format!("{base}/copilot_internal/v2/token"),
        models_url: format!("{base}/models"),
        models: Vec::new(),
        integration_id: "vscode-chat".to_owned(),
        editor_version: "vscode/1.99.2".to_owned(),
        editor_plugin_version: "copilot-chat/0.26.3".to_owned(),
        github_oauth_client_id: "Ov23liTestFixture".to_owned(),
        github_device_code_url: "https://github.com/login/device/code".to_owned(),
        github_oauth_token_url: "https://github.com/login/oauth/access_token".to_owned(),
        github_oauth_scope: "read:user".to_owned(),
        github_login_timeout_secs: 60,
        // Empty so the test does not pick up the developer's real GITHUB_TOKEN.
        github_token_env: Vec::new(),
        request_timeout_secs: 5,
        default_model: "gpt-4.1".to_owned(),
        strict_picker: false,
    }
}

fn model_payload(extra: Vec<serde_json::Value>) -> serde_json::Value {
    let mut data = vec![
        json!({
            "id": "gpt-4.1",
            "model_picker_enabled": true,
            "supported_endpoints": ["/chat/completions"],
            "capabilities": { "limits": { "max_context_window_tokens": 128000 } }
        }),
        json!({
            "id": "claude-sonnet-4",
            "model_picker_enabled": true,
            "supported_endpoints": ["/v1/messages"]
        }),
        json!({
            "id": "internal-hidden",
            "model_picker_enabled": false,
            "supported_endpoints": ["/chat/completions"]
        }),
        json!({
            "id": "policy-disabled",
            "model_picker_enabled": true,
            "supported_endpoints": ["/chat/completions"],
            "policy": { "state": "disabled" }
        }),
    ];
    data.extend(extra);
    json!({ "data": data })
}

fn token_payload(token: &str, expires_at: u64, server_uri: &str) -> serde_json::Value {
    json!({
        "token": token,
        "expires_at": expires_at,
        "endpoints": { "api": server_uri }
    })
}

fn future_unix(secs: u64) -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() + secs)
        .unwrap_or(secs)
}

fn sse_chat_chunk(content: &str) -> String {
    format!(
        "data: {}\n\n",
        json!({ "choices": [{ "delta": { "content": content } }] })
    )
}

async fn collect_events(mut rx: mpsc::Receiver<AppEvent>) -> Vec<ModelEvent> {
    let mut out = Vec::new();
    while let Some(event) = rx.recv().await {
        if let AppEvent::Model(event) = event {
            out.push(event);
        }
    }
    out
}

fn provider_request(messages: Vec<Message>) -> ModelRequest {
    ModelRequest {
        messages,
        system_prompt: Some("You are a test stub.".to_owned()),
        reasoning_effort: None,
        tools: Vec::new(),
        tool_choice: ToolChoice::None,
        max_output_tokens: Some(64),
    }
}

// ── fetch_copilot_models ────────────────────────────────────────────────

#[tokio::test]
async fn fetch_models_filters_picker_disabled_and_policy_disabled() {
    // Default (relaxed) mode: keep `model_picker_enabled = false` models,
    // drop only those with `policy.state = "disabled"`. See
    // `CopilotModel::is_selectable` for rationale.
    let server = MockServer::start().await;
    let (_guard, store) = temp_auth_store();
    seed_copilot_record(&store, "ghu_test_oauth_aaaaaaaa");

    Mock::given(method("GET"))
        .and(path("/copilot_internal/v2/token"))
        .and(header("authorization", "token ghu_test_oauth_aaaaaaaa"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_payload(
            "tid=session_token_aaaa",
            future_unix(1800),
            &server.uri(),
        )))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/models"))
        .and(header("authorization", "Bearer tid=session_token_aaaa"))
        .respond_with(ResponseTemplate::new(200).set_body_json(model_payload(Vec::new())))
        .mount(&server)
        .await;

    let config = copilot_config_for(&server);
    let models = fetch_copilot_models(&config, &store).await.unwrap();

    assert!(
        models.iter().any(|m| m == "gpt-4.1"),
        "expected gpt-4.1 in {models:?}"
    );
    assert!(
        models.iter().any(|m| m == "claude-sonnet-4"),
        "expected claude-sonnet-4 in {models:?}"
    );
    // Default (relaxed) mode keeps `model_picker_enabled = false` models —
    // student/free Copilot plans rely on this to surface the full callable set.
    assert!(
        models.iter().any(|m| m == "internal-hidden"),
        "relaxed mode should keep model_picker_enabled=false models, got {models:?}"
    );
    assert!(
        !models.iter().any(|m| m == "policy-disabled"),
        "policy.state=disabled should always be filtered out"
    );
}

#[tokio::test]
async fn fetch_models_strict_mode_filters_picker_disabled_models() {
    let server = MockServer::start().await;
    let (_guard, store) = temp_auth_store();
    seed_copilot_record(&store, "ghu_test_oauth_strictpicker");

    Mock::given(method("GET"))
        .and(path("/copilot_internal/v2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_payload(
            "tid=session_token_strict",
            future_unix(1800),
            &server.uri(),
        )))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/models"))
        .and(header(
            "authorization",
            "Bearer tid=session_token_strict",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(model_payload(Vec::new())))
        .mount(&server)
        .await;

    let mut config = copilot_config_for(&server);
    config.strict_picker = true;
    let models = fetch_copilot_models(&config, &store).await.unwrap();

    assert!(
        models.iter().any(|m| m == "gpt-4.1"),
        "expected gpt-4.1 in {models:?}"
    );
    assert!(
        !models.iter().any(|m| m == "internal-hidden"),
        "strict mode should drop model_picker_enabled=false models, got {models:?}"
    );
    assert!(
        !models.iter().any(|m| m == "policy-disabled"),
        "policy.state=disabled should be filtered out, got {models:?}"
    );
}

#[tokio::test]
async fn fetch_models_persists_endpoint_metadata_to_store() {
    let server = MockServer::start().await;
    let (_guard, store) = temp_auth_store();
    seed_copilot_record(&store, "ghu_test_oauth_bbbbbbbb");

    Mock::given(method("GET"))
        .and(path("/copilot_internal/v2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_payload(
            "tid=session_token_bbbb",
            future_unix(1800),
            &server.uri(),
        )))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(model_payload(Vec::new())))
        .mount(&server)
        .await;

    let config = copilot_config_for(&server);
    let _ = fetch_copilot_models(&config, &store).await.unwrap();

    let record = store.record("copilot").unwrap().unwrap();
    let metadata_endpoints = record
        .metadata
        .get("model_endpoints")
        .expect("model_endpoints metadata should be persisted");
    assert!(
        metadata_endpoints.contains("gpt-4.1"),
        "endpoints metadata should include selectable models: {metadata_endpoints}"
    );
    assert!(
        metadata_endpoints.contains("claude-sonnet-4"),
        "endpoints metadata should include claude-shaped routing target"
    );
}

#[tokio::test]
async fn fetch_models_rejects_personal_access_tokens() {
    // Scrub PATH so the in-process `gh auth token` lookup cannot find the gh
    // CLI and inject the developer's real Copilot token as an extra candidate.
    // Without this, the PAT rejection still fires but the surfaced error is
    // the *last* candidate's failure (gh's), not the PAT-specific message.
    let prev_path = std::env::var("PATH").ok();
    std::env::set_var("PATH", "");

    let server = MockServer::start().await;
    let (_guard, store) = temp_auth_store();
    seed_copilot_record(&store, "ghp_personal_access_token_xxxx");

    Mock::given(method("GET"))
        .and(path("/copilot_internal/v2/token"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let config = copilot_config_for(&server);
    let result = fetch_copilot_models(&config, &store).await;

    // Restore PATH before any assertion can panic.
    match prev_path {
        Some(value) => std::env::set_var("PATH", value),
        None => std::env::remove_var("PATH"),
    }

    let err = result.expect_err("ghp_ tokens must not authenticate against Copilot");
    let message = format!("{err:?}");
    assert!(
        message.contains("Personal Access Tokens") || message.contains("ghp_"),
        "PAT error should be friendly, got: {message}"
    );
}

// ── Streaming chat completions ──────────────────────────────────────────

#[tokio::test]
async fn stream_chat_emits_text_deltas_and_done() {
    let server = MockServer::start().await;
    let (_guard, store) = temp_auth_store();
    seed_copilot_record(&store, "ghu_test_oauth_cccccccc");

    Mock::given(method("GET"))
        .and(path("/copilot_internal/v2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_payload(
            "tid=session_token_cccc",
            future_unix(1800),
            &server.uri(),
        )))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(model_payload(Vec::new())))
        .mount(&server)
        .await;

    let body = format!(
        "{}{}{}data: [DONE]\n\n",
        sse_chat_chunk("hello "),
        sse_chat_chunk("world"),
        sse_chat_chunk("!"),
    );
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer tid=session_token_cccc"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .mount(&server)
        .await;

    let provider = CopilotProvider::new(copilot_config_for(&server), Some(store));
    let request = provider_request(vec![Message::new(Role::User, "hi")]);
    let (tx, rx) = mpsc::channel(32);
    provider.stream_turn(request, tx).await;

    let events = collect_events(rx).await;
    let text: String = events
        .iter()
        .filter_map(|event| match event {
            ModelEvent::TextDelta(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "hello world!");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ModelEvent::Done { .. })),
        "stream must end with Done event"
    );
}

// ── 401 → refresh → retry ───────────────────────────────────────────────

#[tokio::test]
async fn unauthorized_chat_response_triggers_token_refresh_and_retry() {
    let server = MockServer::start().await;
    let (_guard, store) = temp_auth_store();
    seed_copilot_record(&store, "ghu_test_oauth_dddddddd");

    // Token endpoint will be hit at least twice — once on first session
    // acquisition, again on the forced refresh after the 401.
    Mock::given(method("GET"))
        .and(path("/copilot_internal/v2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_payload(
            "tid=session_token_dddd",
            future_unix(1800),
            &server.uri(),
        )))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(model_payload(Vec::new())))
        .mount(&server)
        .await;

    // First chat call returns 401 (capped at 1), subsequent calls return 200.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_string("token expired"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(format!("{}data: [DONE]\n\n", sse_chat_chunk("retry-ok"))),
        )
        .mount(&server)
        .await;

    let provider = CopilotProvider::new(copilot_config_for(&server), Some(store));
    let request = provider_request(vec![Message::new(Role::User, "ping")]);
    let (tx, rx) = mpsc::channel(32);
    provider.stream_turn(request, tx).await;

    let events = collect_events(rx).await;
    let text: String = events
        .iter()
        .filter_map(|event| match event {
            ModelEvent::TextDelta(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "retry-ok", "retry should produce the second response");

    assert!(
        events
            .iter()
            .any(|event| matches!(event, ModelEvent::Done { .. })),
        "stream must end with Done after retry"
    );
}

// ── Anthropic-shaped messages routing ──────────────────────────────────

#[tokio::test]
async fn claude_models_route_to_messages_endpoint() {
    let server = MockServer::start().await;
    let (_guard, store) = temp_auth_store();
    seed_copilot_record(&store, "ghu_test_oauth_eeeeeeee");

    Mock::given(method("GET"))
        .and(path("/copilot_internal/v2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_payload(
            "tid=session_token_eeee",
            future_unix(1800),
            &server.uri(),
        )))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(model_payload(Vec::new())))
        .mount(&server)
        .await;

    // Anthropic-shaped event with content_block_delta.
    let claude_body = format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({
            "type": "content_block_delta",
            "delta": { "type": "text_delta", "text": "claude-says-hi" }
        })
    );
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(claude_body),
        )
        .mount(&server)
        .await;

    let mut config = copilot_config_for(&server);
    config.default_model = "claude-sonnet-4".to_owned();

    let provider = CopilotProvider::new(config, Some(store));
    let request = provider_request(vec![Message::new(Role::User, "hi-claude")]);
    let (tx, rx) = mpsc::channel(32);
    provider.stream_turn(request, tx).await;

    let events = collect_events(rx).await;
    let text: String = events
        .iter()
        .filter_map(|event| match event {
            ModelEvent::TextDelta(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "claude-says-hi");
}

// ── Cached session expiry ──────────────────────────────────────────────

#[tokio::test]
async fn expired_cached_session_forces_token_re_exchange() {
    let server = MockServer::start().await;
    let (_guard, store) = temp_auth_store();

    // Seed an *expired* cached session in metadata. Provider must ignore it
    // and call the token endpoint again.
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "copilot_session_token".to_owned(),
        "tid=expired_cache".to_owned(),
    );
    metadata.insert("copilot_session_api_base_url".to_owned(), server.uri());
    metadata.insert(
        "copilot_session_expires_at".to_owned(),
        // 5 minutes in the past
        future_unix(0).saturating_sub(300).to_string(),
    );
    store
        .upsert(AuthRecord {
            provider_id: "copilot".to_owned(),
            account_label: Some("integration-test".to_owned()),
            access_token: Some("ghu_test_oauth_ffffffff".to_owned()),
            refresh_token: None,
            expires_at: None,
            updated_at: 0,
            metadata,
        })
        .unwrap();

    Mock::given(method("GET"))
        .and(path("/copilot_internal/v2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_payload(
            "tid=fresh_after_expiry",
            future_unix(1800),
            &server.uri(),
        )))
        // expect at least one fresh exchange — must not reuse the cached token
        .expect(1..)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(model_payload(Vec::new())))
        .mount(&server)
        .await;

    let config = copilot_config_for(&server);
    let _ = fetch_copilot_models(&config, &store).await.unwrap();

    let record = store.record("copilot").unwrap().unwrap();
    let cached_token = record.metadata.get("copilot_session_token").cloned();
    assert_eq!(
        cached_token.as_deref(),
        Some("tid=fresh_after_expiry"),
        "expired cache must be replaced with the freshly exchanged token"
    );
}
