//! Freemodel.dev provider integration.
//!
//! End-user binaries reach freemodel.dev through the artui Cloudflare
//! Worker relay (see `cloudflare/`). The relay holds the upstream API key
//! server-side and proxies the OpenAI-format routes
//! (`/v1/chat/completions`, `/v1/models`), so the binary itself ships no
//! credentials. artui reuses [`super::OpenAiCompatProvider`] for the chat
//! surface — this module only owns the runtime model-discovery helper that
//! hits `GET {base_url}/models` so the `/model` picker can show every
//! model the relay exposes for the current upstream key. The configured
//! `base_url` already includes the `/v1` prefix, so this helper appends
//! `/models` directly.
//!
//! Discovery is best-effort: a network failure or HTTP error returns `Ok` of
//! an empty list rather than an error, because the configured
//! [`crate::config::FreemodelConfig::default_model`] is always added back in
//! at the call site. The picker stays usable even when the user is offline
//! or the relay is briefly unavailable.

use anyhow::Context;
use serde::Deserialize;

use crate::config::FreemodelConfig;

/// Builds the [`crate::config::OpenAiCompatConfig`] used to construct a
/// freemodel-flavoured `OpenAiCompatProvider`. The credential lookup is
/// pinned to `provider_id = "freemodel"` so the OpenAI key chain is never
/// consulted. When the binary points at the default Cloudflare relay,
/// `resolve_credential("freemodel", _)` typically returns `None` and the
/// provider sends no Authorization header — the relay injects it
/// upstream. Power users who set `FREEMODEL_API_KEY` (or
/// `ARTUI_FREEMODEL_API_KEY`) and override `base_url` to
/// `https://api.freemodel.dev/v1` get the env-var value forwarded straight
/// to the upstream.
pub fn openai_compat_config(config: &FreemodelConfig) -> crate::config::OpenAiCompatConfig {
    crate::config::OpenAiCompatConfig {
        base_url: config.base_url.clone(),
        // The compat provider falls back to `std::env::var(api_key_env)` when
        // `resolve_credential` returns nothing. We point it at
        // `FREEMODEL_API_KEY` so a user-supplied env var still wires up
        // correctly when bypassing the relay.
        api_key_env: "FREEMODEL_API_KEY".to_owned(),
        default_model: config.default_model.clone(),
        credential_provider_id: "freemodel".to_owned(),
    }
}

/// Discover the model id list exposed by `{base_url}/models`. Returns an
/// empty vec on transport, HTTP, or parse failure — the picker is expected
/// to combine this with the configured default. The `base_url` is expected to
/// already include the `/v1` prefix (matching the OpenAI-compat convention),
/// so we append `/models` directly.
pub async fn discover_models(config: &FreemodelConfig) -> anyhow::Result<Vec<String>> {
    let timeout = std::time::Duration::from_secs(config.request_timeout_secs.max(1));
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .context("failed to construct freemodel http client")?;
    let url = format!("{}/models", config.base_url.trim_end_matches('/'));

    let mut req = client.get(&url);
    if let Some(api_key) = crate::auth::resolve_credential("freemodel", None)
        .filter(|value| !value.trim().is_empty())
    {
        req = req.header("Authorization", format!("Bearer {api_key}"));
    }

    let response = match req.send().await {
        Ok(response) => response,
        Err(_) => return Ok(Vec::new()),
    };
    if !response.status().is_success() {
        return Ok(Vec::new());
    }

    let body = match response.json::<ModelsResponse>().await {
        Ok(body) => body,
        Err(_) => return Ok(Vec::new()),
    };

    Ok(body
        .data
        .into_iter()
        .filter_map(|entry| {
            let trimmed = entry.id.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_owned())
            }
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_v1_models_response() {
        let body = r#"{"data":[
            {"created":1626777600,"id":"gpt-5.5","object":"model","owned_by":"freemodel","supported_endpoint_types":["openai"]},
            {"created":1626777600,"id":"gpt-5.4","object":"model","owned_by":"freemodel","supported_endpoint_types":["openai"]},
            {"created":1626777600,"id":"  ","object":"model","owned_by":"freemodel","supported_endpoint_types":["openai"]}
        ],"object":"list"}"#;
        let parsed: ModelsResponse = serde_json::from_str(body).unwrap();
        let ids: Vec<String> = parsed
            .data
            .into_iter()
            .filter_map(|entry| {
                let trimmed = entry.id.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_owned())
                }
            })
            .collect();
        assert_eq!(ids, vec!["gpt-5.5".to_owned(), "gpt-5.4".to_owned()]);
    }

    #[test]
    fn openai_compat_config_pins_credential_provider_id() {
        let cfg = FreemodelConfig::default();
        let compat = openai_compat_config(&cfg);
        assert_eq!(compat.credential_provider_id, "freemodel");
        assert_eq!(compat.api_key_env, "FREEMODEL_API_KEY");
        assert_eq!(compat.base_url, cfg.base_url);
        assert_eq!(compat.default_model, cfg.default_model);
    }
}
