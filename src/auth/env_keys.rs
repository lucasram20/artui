//! Pi-style central credential resolver.
//!
//! Each provider has a fixed list of environment variable names it answers
//! to (OAuth-shape names take precedence over plain API-key names). The
//! resolver also checks the artui `AuthStore` for explicitly logged-in
//! credentials, so a user who runs `/login openai_compat` overrides the
//! environment chain.
//!
//! Modeled on
//! <https://github.com/earendil-works/pi/blob/main/packages/ai/src/env-api-keys.ts>.

use crate::auth::AuthStore;

/// Environment variables checked per provider, in priority order.
/// First non-empty value wins.
pub fn known_env_keys(provider_id: &str) -> &'static [&'static str] {
    match provider_id {
        // Pi precedence: OAuth before plain key.
        "openai_compat" | "openai" => &["OPENAI_OAUTH_TOKEN", "OPENAI_API_KEY"],
        "anthropic" => &["ANTHROPIC_OAUTH_TOKEN", "ANTHROPIC_API_KEY"],
        "openai_account" => &["OPENAI_OAUTH_TOKEN"],
        // GitHub Copilot tokens are handled separately via copilot.rs because
        // they require an exchange step. Listed for completeness.
        "copilot" => &["GH_TOKEN", "GITHUB_TOKEN"],
        // Ollama is local-only by default; we still allow OLLAMA_API_KEY for
        // gateway proxies.
        "ollama" => &["OLLAMA_API_KEY"],
        // Hosted default provider: relay injects the maintainer key server-side,
        // so the binary normally sends no Authorization header. These env vars
        // are for power users who bypass the relay (see cloudflare/README.md).
        "freemodel" => &["FREEMODEL_API_KEY", "ARTUI_FREEMODEL_API_KEY"],
        _ => &[],
    }
}

/// Resolve a credential for `provider_id`. Order: AuthStore (explicit login)
/// → environment variables. Returns `None` if nothing is available — the
/// caller is expected to handle the no-credential case (the freemodel relay
/// path is one such caller: it talks to the relay anonymously).
pub fn resolve_credential(provider_id: &str, store: Option<&AuthStore>) -> Option<String> {
    if let Some(store) = store {
        if let Ok(Some(record)) = store.record(provider_id) {
            if let Some(token) = record.access_token.filter(|t| !t.trim().is_empty()) {
                return Some(token);
            }
        }
    }
    for env_name in known_env_keys(provider_id) {
        if let Ok(value) = std::env::var(env_name) {
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// Diagnostic helper: report the env name that satisfied a lookup, or None.
pub fn satisfying_env_key(provider_id: &str) -> Option<&'static str> {
    for env_name in known_env_keys(provider_id) {
        if let Ok(value) = std::env::var(env_name) {
            if !value.trim().is_empty() {
                return Some(env_name);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_envs<F: FnOnce()>(pairs: &[(&str, Option<&str>)], f: F) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let saved: Vec<_> = pairs
            .iter()
            .map(|(k, _)| (*k, std::env::var(*k).ok()))
            .collect();
        for (k, v) in pairs {
            match v {
                Some(value) => std::env::set_var(k, value),
                None => std::env::remove_var(k),
            }
        }
        f();
        for (k, prev) in saved {
            match prev {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }

    #[test]
    fn unknown_provider_has_no_keys() {
        assert!(known_env_keys("nonexistent").is_empty());
    }

    #[test]
    fn openai_prefers_oauth_token() {
        let keys = known_env_keys("openai_compat");
        assert_eq!(keys[0], "OPENAI_OAUTH_TOKEN");
        assert_eq!(keys[1], "OPENAI_API_KEY");
    }

    #[test]
    fn anthropic_prefers_oauth_token() {
        let keys = known_env_keys("anthropic");
        assert_eq!(keys[0], "ANTHROPIC_OAUTH_TOKEN");
    }

    #[test]
    fn resolve_picks_first_non_empty_env() {
        with_envs(
            &[
                ("OPENAI_OAUTH_TOKEN", None),
                ("OPENAI_API_KEY", Some("sk-test-from-env")),
            ],
            || {
                let resolved = resolve_credential("openai_compat", None);
                assert_eq!(resolved.as_deref(), Some("sk-test-from-env"));
            },
        );
    }

    #[test]
    fn resolve_returns_none_when_unset() {
        with_envs(
            &[("OPENAI_API_KEY", None), ("OPENAI_OAUTH_TOKEN", None)],
            || {
                assert!(resolve_credential("openai_compat", None).is_none());
            },
        );
    }
}
