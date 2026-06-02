use crate::config::AppConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthRequirement {
    None,
    ApiKey,
    Account,
}

impl AuthRequirement {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "no login",
            Self::ApiKey => "API key",
            Self::Account => "account login",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelListStrategy {
    LocalDiscovery,
    Configured,
    ProviderEndpoint,
}

impl ModelListStrategy {
    pub fn label(self) -> &'static str {
        match self {
            Self::LocalDiscovery => "local discovery",
            Self::Configured => "configured",
            Self::ProviderEndpoint => "provider endpoint",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderMetadata {
    pub id: &'static str,
    pub display_name: &'static str,
    pub auth_requirement: AuthRequirement,
    pub model_list_strategy: ModelListStrategy,
    pub streaming: bool,
}

pub const PROVIDERS: [ProviderMetadata; 5] = [
    ProviderMetadata {
        id: "freemodel",
        display_name: "artui",
        // Relay-backed hosted provider; no `/login` required (optional
        // FREEMODEL_API_KEY when bypassing the relay). Internal id stays
        // `freemodel`; UI label is `artui`.
        auth_requirement: AuthRequirement::None,
        model_list_strategy: ModelListStrategy::ProviderEndpoint,
        streaming: true,
    },
    ProviderMetadata {
        id: "ollama",
        display_name: "Ollama",
        auth_requirement: AuthRequirement::None,
        model_list_strategy: ModelListStrategy::LocalDiscovery,
        streaming: true,
    },
    ProviderMetadata {
        id: "openai_compat",
        display_name: "OpenAI-compatible API",
        auth_requirement: AuthRequirement::ApiKey,
        model_list_strategy: ModelListStrategy::Configured,
        streaming: false,
    },
    ProviderMetadata {
        id: "copilot",
        display_name: "GitHub Copilot",
        auth_requirement: AuthRequirement::Account,
        model_list_strategy: ModelListStrategy::ProviderEndpoint,
        streaming: true,
    },
    ProviderMetadata {
        id: "openai_account",
        display_name: "OpenAI Codex",
        auth_requirement: AuthRequirement::Account,
        model_list_strategy: ModelListStrategy::ProviderEndpoint,
        streaming: false,
    },
];

/// Subset of `PROVIDERS` shown in the `/login` picker. Only providers that
/// require an interactive account login appear here — Ollama runs locally
/// and `openai_compat` resolves credentials from environment variables, so
/// neither has a meaningful login flow to surface.
pub const LOGIN_PROVIDERS: [ProviderMetadata; 2] = [
    ProviderMetadata {
        id: "copilot",
        display_name: "GitHub Copilot",
        auth_requirement: AuthRequirement::Account,
        model_list_strategy: ModelListStrategy::ProviderEndpoint,
        streaming: true,
    },
    ProviderMetadata {
        id: "openai_account",
        display_name: "OpenAI Codex",
        auth_requirement: AuthRequirement::Account,
        model_list_strategy: ModelListStrategy::ProviderEndpoint,
        streaming: false,
    },
];

pub fn provider_metadata(id: &str) -> Option<&'static ProviderMetadata> {
    PROVIDERS.iter().find(|provider| provider.id == id)
}

/// User-visible provider label (picker headers, input chrome, status).
pub fn provider_display_name(id: &str) -> &str {
    provider_metadata(id)
        .map(|provider| provider.display_name)
        .unwrap_or(id)
}

pub fn configured_model<'a>(config: &'a AppConfig, provider_id: &str) -> &'a str {
    match provider_id {
        "freemodel" => config.providers.freemodel.default_model.as_str(),
        "ollama" => config.providers.ollama.default_model.as_str(),
        "openai_compat" => config.providers.openai_compat.default_model.as_str(),
        "copilot" => config.providers.copilot.default_model.as_str(),
        "openai_account" => config.providers.openai_account.default_model.as_str(),
        _ => "",
    }
}
