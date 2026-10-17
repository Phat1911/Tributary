//! Environment-driven configuration.
//!
//! Every knob is a `TRIBUTARY_*` env var. M2 change: upstream credentials
//! no longer live here — they are pool-scoped provider keys in the DB,
//! sealed under the master key (rule 2). `serve` therefore requires
//! `TRIBUTARY_MASTER_KEY` instead of `TRIBUTARY_OPENAI_API_KEY`.

use std::env;
use std::fmt;

use tributary_crypto::MasterKey;

/// Gateway configuration loaded from the environment.
#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    /// Master key for provider-key envelope decryption (env, never DB).
    pub master_key: MasterKey,
    pub bind: String,
    /// Default model allowlist for new pools created without one, and the
    /// fallback list for `GET /v1/models` on unrestricted pools. Model
    /// identities live in config, never business logic (SPEC §15).
    pub default_models: Vec<String>,
    /// Emit structured JSON logs (SPEC §13 "JSON logs (tracing)").
    pub log_json: bool,
}

impl fmt::Debug for Config {
    /// Manual Debug impl: the master key and any DSN password must never
    /// reach logs. (MasterKey redacts itself, but defense in depth is cheap.)
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &"<redacted>")
            .field("master_key", &"<redacted>")
            .field("bind", &self.bind)
            .field("default_models", &self.default_models)
            .field("log_json", &self.log_json)
            .finish()
    }
}

impl Config {
    /// Full config for `serve` (requires DB URL + master key).
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            database_url: env_required("TRIBUTARY_DATABASE_URL")?,
            master_key: MasterKey::from_env()?,
            bind: env::var("TRIBUTARY_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string()),
            default_models: model_list(),
            log_json: env::var("TRIBUTARY_LOG_FORMAT")
                .is_ok_and(|v| v.eq_ignore_ascii_case("json")),
        })
    }

    /// DB-only config for `migrate` / `create-admin` (no master key needed).
    pub fn database_url_from_env() -> anyhow::Result<String> {
        env_required("TRIBUTARY_DATABASE_URL")
    }
}

/// Reads a required, non-empty environment variable.
fn env_required(name: &str) -> anyhow::Result<String> {
    let value = env::var(name)
        .map_err(|_| anyhow::anyhow!("required environment variable {name} is not set"))?;
    if value.trim().is_empty() {
        anyhow::bail!("required environment variable {name} is empty");
    }
    Ok(value)
}

/// Parses `TRIBUTARY_MODELS` (comma-separated), defaulting to one safe model.
fn model_list() -> Vec<String> {
    match env::var("TRIBUTARY_MODELS") {
        Ok(raw) => raw
            .split(',')
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_string)
            .collect(),
        // Config default, overridable via env — model identities never live in code.
        Err(_) => vec!["gpt-4o-mini".to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_leaks_secrets() {
        let config = Config {
            database_url: "postgres://user:supersecret@host/db".to_string(),
            master_key: MasterKey::generate(),
            bind: "127.0.0.1:8080".to_string(),
            default_models: model_list(),
            log_json: false,
        };
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("supersecret"));
        assert!(!rendered.contains("redacted\"")); // fields show <redacted>
        assert!(rendered.contains("gpt-4o-mini"));
    }
}
