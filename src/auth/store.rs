use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;

const AUTH_FILE_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct AuthStore {
    path: PathBuf,
}

impl AuthStore {
    pub fn from_config(config: &AppConfig) -> Option<Self> {
        let path = config
            .auth_storage_path
            .clone()
            .or_else(crate::util::paths::auth_store_path)?;
        Some(Self::new(path))
    }

    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Vec<AuthRecord>> {
        Ok(self.load_file()?.records.into_values().collect())
    }

    pub fn upsert(&self, mut record: AuthRecord) -> Result<()> {
        let mut file = self.load_file()?;
        record.provider_id = normalize_provider_id(&record.provider_id);
        record.updated_at = unix_timestamp();
        file.records.insert(record.provider_id.clone(), record);
        self.save_file(&file)
    }

    pub fn remove(&self, provider_id: &str) -> Result<bool> {
        let mut file = self.load_file()?;
        let removed = file
            .records
            .remove(&normalize_provider_id(provider_id))
            .is_some();
        if removed {
            self.save_file(&file)?;
        }
        Ok(removed)
    }

    pub fn status(&self, provider_id: &str) -> Result<AuthStatus> {
        let file = self.load_file()?;
        Ok(file
            .records
            .get(&normalize_provider_id(provider_id))
            .map(AuthRecord::status)
            .unwrap_or(AuthStatus::NotConnected))
    }

    pub fn record(&self, provider_id: &str) -> Result<Option<AuthRecord>> {
        let file = self.load_file()?;
        Ok(file
            .records
            .get(&normalize_provider_id(provider_id))
            .cloned())
    }

    pub fn provider_statuses(
        &self,
        provider_ids: impl IntoIterator<Item = String>,
    ) -> Result<Vec<ProviderAuthStatus>> {
        let file = self.load_file()?;
        Ok(provider_ids
            .into_iter()
            .map(|provider_id| {
                let normalized = normalize_provider_id(&provider_id);
                let status = file
                    .records
                    .get(&normalized)
                    .map(AuthRecord::status)
                    .unwrap_or(AuthStatus::NotConnected);
                ProviderAuthStatus {
                    provider_id: normalized,
                    status,
                }
            })
            .collect())
    }

    fn load_file(&self) -> Result<AuthFile> {
        if !self.path.exists() {
            return Ok(AuthFile::default());
        }

        let content = fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read auth store at {}", self.path.display()))?;
        let mut file: AuthFile = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse auth store at {}", self.path.display()))?;
        if file.version == 0 {
            file.version = AUTH_FILE_VERSION;
        }
        Ok(file)
    }

    fn save_file(&self, file: &AuthFile) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create auth store directory {}", parent.display())
            })?;
        }

        let tmp_path = self.path.with_extension("json.tmp");
        let content = serde_json::to_vec_pretty(file).context("failed to serialize auth store")?;
        write_private_file(&tmp_path, &content)
            .with_context(|| format!("failed to write auth store at {}", tmp_path.display()))?;
        fs::rename(&tmp_path, &self.path).with_context(|| {
            format!(
                "failed to replace auth store {} with {}",
                self.path.display(),
                tmp_path.display()
            )
        })?;
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AuthFile {
    #[serde(default = "auth_file_version")]
    version: u32,
    #[serde(default)]
    records: BTreeMap<String, AuthRecord>,
}

fn auth_file_version() -> u32 {
    AUTH_FILE_VERSION
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AuthRecord {
    pub provider_id: String,
    #[serde(default)]
    pub account_label: Option<String>,
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_at: Option<u64>,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl AuthRecord {
    pub fn status(&self) -> AuthStatus {
        if self.access_token.as_deref().unwrap_or_default().is_empty()
            && self.refresh_token.as_deref().unwrap_or_default().is_empty()
        {
            return AuthStatus::NotConnected;
        }

        match self.expires_at {
            Some(expires_at) if expires_at <= unix_timestamp() => AuthStatus::Expired,
            _ => AuthStatus::Connected,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStatus {
    Connected,
    Expired,
    NotConnected,
}

impl AuthStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Expired => "expired",
            Self::NotConnected => "not connected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAuthStatus {
    pub provider_id: String,
    pub status: AuthStatus,
}

impl std::fmt::Debug for AuthRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AuthRecord")
            .field("provider_id", &self.provider_id)
            .field("account_label", &self.account_label)
            .field("access_token", &redacted(self.access_token.as_deref()))
            .field("refresh_token", &redacted(self.refresh_token.as_deref()))
            .field("expires_at", &self.expires_at)
            .field("updated_at", &self.updated_at)
            .field("metadata", &self.metadata.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl std::fmt::Display for AuthRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, formatter)
    }
}

fn redacted(value: Option<&str>) -> &'static str {
    match value {
        Some(value) if !value.is_empty() => "<redacted>",
        _ => "<empty>",
    }
}

fn normalize_provider_id(provider_id: &str) -> String {
    provider_id.trim().to_ascii_lowercase()
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(unix)]
fn write_private_file(path: &Path, content: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(content)?;
    file.sync_all()
}

#[cfg(not(unix))]
fn write_private_file(path: &Path, content: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    file.write_all(content)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_auth_path(name: &str) -> PathBuf {
        let unique = format!(
            "artui-auth-test-{}-{}",
            name,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(unique).join("auth.json")
    }

    fn record(provider_id: &str, expires_at: Option<u64>) -> AuthRecord {
        AuthRecord {
            provider_id: provider_id.to_owned(),
            account_label: Some("user@example.test".to_owned()),
            access_token: Some("secret-access".to_owned()),
            refresh_token: Some("secret-refresh".to_owned()),
            expires_at,
            updated_at: 0,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn missing_file_loads_empty_store() {
        let store = AuthStore::new(temp_auth_path("missing"));
        let records = store.load().unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn serializes_and_reads_provider_scoped_records() {
        let store = AuthStore::new(temp_auth_path("roundtrip"));
        store.upsert(record("Copilot", None)).unwrap();

        let records = store.load().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].provider_id, "copilot");
        assert_eq!(store.status("copilot").unwrap(), AuthStatus::Connected);
    }

    #[test]
    fn corrupt_file_returns_parse_error_without_secret_body() {
        let path = temp_auth_path("corrupt");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "secret-token-not-json").unwrap();

        let error = AuthStore::new(path).load().unwrap_err().to_string();
        assert!(error.contains("failed to parse auth store"));
        assert!(!error.contains("secret-token-not-json"));
    }

    #[test]
    fn display_redacts_tokens() {
        let rendered = record("copilot", None).to_string();
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("secret-access"));
        assert!(!rendered.contains("secret-refresh"));
    }

    #[test]
    fn debug_redacts_tokens() {
        let rendered = format!("{:?}", record("copilot", None));
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("secret-access"));
        assert!(!rendered.contains("secret-refresh"));
    }

    #[test]
    fn expired_records_report_expired_status() {
        let expired = record("copilot", Some(unix_timestamp().saturating_sub(1)));
        assert_eq!(expired.status(), AuthStatus::Expired);
    }

    #[cfg(unix)]
    #[test]
    fn saved_auth_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let path = temp_auth_path("permissions");
        let store = AuthStore::new(path.clone());
        store.upsert(record("copilot", None)).unwrap();

        let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
