//! Environment-driven configuration (M1: no config file yet).
//!
//! Every knob is a `TRIBUTARY_*` env var. Provider/model identities live in
//! config, never in business logic (SPEC §15 extensibility NFR).

use std::env;
use std::fmt;

/// Gateway configuration loaded from the environment.
///
/// The upstream API key is held for the process lifetime in M1 (there is no
/// key vault until M2's crypto crate) and is masked in `Debug` output so it
/// can never leak into logs (AGENTS secrets convention).
#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub openai_api_key: String,
    pub openai_base_url: String,
    pub bind: String,
    /// Static model list served by `GET /v1/models` (pool allowlists arrive M2).
    pub models: Vec<String>,
    /// Emit structured JSON logs (SPEC §13 "JSON logs (tracing)").
    pub log_json: bool,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &self.database_url)
            .field("openai_api_key", &"<redacted>")
            .field("openai_base_url", &self.openai_base_url)
            .field("bind", &self.bind)
            .field("models", &self.models)
            .field("log_json", &self.log_json)
            .finish()
    }
}

impl Config {
    /// Full config for `serve` (requires the upstream key).
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            database_url: env_required("TRIBUTARY_DATABASE_URL")?,
            openai_api_key: env_required("TRIBUTARY_OPENAI_API_KEY")?,
            openai_base_url: env::var("TRIBUTARY_OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string())
                .trim_end_matches('/')
                .to_string(),
            bind: env::var("TRIBUTARY_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string()),
            models: model_list(),
            log_json: env::var("TRIBUTARY_LOG_FORMAT")
                .is_ok_and(|v| v.eq_ignore_ascii_case("json")),
        })
    }

    /// DB-only config for `migrate` / `seed-key` (no upstream key needed).
    pub fn database_url_from_env() -> anyhow::Result<String> {
        env_required("TRIBUTARY_DATABASE_URL")
    }
}

fn env_required(name: &str) -> anyhow::Result<String> {
    let value = env::var(name)
        .map_err(|_| anyhow::anyhow!("required environment variable {name} is not set"))?;
    if value.trim().is_empty() {
        anyhow::bail!("required environment variable {name} is empty");
    }
    Ok(value)
}

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
