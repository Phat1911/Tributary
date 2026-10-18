//! BYOK purpose-alignment classifier (D28, M4).
//!
//! Contract decisions, all from D28:
//! - **Raw HTTP, version-pinned, no vendor SDKs** — [`JevClassifier`] posts
//!   a fixed JSON shape to a fixed path ([`ALIGN_ENDPOINT`]) via reqwest.
//!   The model id travels in org config, never code (SPEC §15).
//! - **Tolerant parsing** — [`parse_alignment`] accepts the observed
//!   response dialects: `alignment`/`score`/`alignment_score`, top-level or
//!   nested under `result`, as numbers or numeric strings, clamped to 0..=1.
//! - **Fail-open** — any transport/parse failure surfaces as an `Err` the
//!   CALLER turns into "no score" (never a blocked or failed request); the
//!   auxiliary path can never take the data plane down.
//! - **Churn defenses** — save-time probe + fixture contract tests
//!   (`tests/classifier_contract.rs`) keep us honest when the vendor drifts.
//!
//! The provider trait exists so future classifiers (or the CI smoke job)
//! plug in without touching the scoring pipeline (SPEC §15 trait seams).

use std::future::Future;
use std::pin::Pin;

use serde_json::Value;
use zeroize::Zeroizing;

/// The version-pinned scoring endpoint appended to the configured
/// `base_url`. "Version-pinned" means: when the vendor ships a v2, we add a
/// NEW constant and an org-config switch — we never silently follow a
/// redirect or floating "latest" path (D28 churn defense).
pub const ALIGN_ENDPOINT: &str = "/v1/align";

/// Budget for one classification call (async, off the hot path, but still
/// bounded — a hung vendor must not leak tasks forever).
pub const SCORE_TIMEOUT_SECS: u64 = 10;

/// One scoring request. `text` is the prompt material — it is sent to the
/// org's classifier and NEVER persisted (D28: scores, not prompts).
pub struct ScoreRequest<'a> {
    pub model: &'a str,
    pub purpose: &'a str,
    pub text: &'a str,
}

/// A successful classification: alignment in 0..=1 (1 = fully on-purpose).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scored {
    pub alignment: f64,
}

/// Why a classification attempt failed. All variants are fail-open at the
/// call site — logged, never user-visible.
#[derive(Debug, thiserror::Error)]
pub enum ClassifierError {
    #[error("classifier request failed: {0}")]
    Transport(String),
    #[error("classifier returned HTTP {0}")]
    Status(u16),
    #[error("classifier response unparseable")]
    Unparseable,
}

/// The seam future type: `async fn` in traits is not object-safe, so
/// implementors return this pinned boxed future instead (the gateway holds
/// providers behind `&dyn` when wiring the smoke job / tests).
pub type ScoreFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Scored, ClassifierError>> + Send + 'a>>;

/// A purpose-alignment classifier the org configured (D28). One
/// implementation ships today (Jev); the trait is the extension point.
pub trait ClassifierProvider: Send + Sync {
    /// Scores one request. Implementations must be side-effect free apart
    /// from the outbound HTTP call.
    fn score<'a>(&'a self, req: ScoreRequest<'a>) -> ScoreFuture<'a>;
}

/// The Jev adapter: raw HTTP, no SDK. Constructed per use from the org's
/// stored config; the API key lives in `Zeroizing` and is dropped with the
/// adapter (never cached, AGENTS secrets convention).
pub struct JevClassifier {
    http: reqwest::Client,
    base_url: String,
    model: String,
    api_key: Zeroizing<String>,
}

impl JevClassifier {
    /// Builds a provider bound to one org's config snapshot.
    pub fn new(
        http: reqwest::Client,
        base_url: &str,
        model: &str,
        api_key: Zeroizing<String>,
    ) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key,
        }
    }

    /// Plain async implementation — the trait adapter wraps it in a boxed
    /// future (kept separate so unit tests can await it directly).
    async fn score_impl(&self, req: ScoreRequest<'_>) -> Result<Scored, ClassifierError> {
        let url = format!("{}{}", self.base_url, ALIGN_ENDPOINT);
        let payload = serde_json::json!({
            "model": self.model,
            "purpose": req.purpose,
            "text": req.text,
        });
        let response = self
            .http
            .post(&url)
            .bearer_auth(self.api_key.as_str())
            .timeout(std::time::Duration::from_secs(SCORE_TIMEOUT_SECS))
            .json(&payload)
            .send()
            .await
            .map_err(|e| ClassifierError::Transport(e.to_string()))?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            // The body may carry a vendor error message — useful in logs,
            // never surfaced to members.
            return Err(ClassifierError::Status(status));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| ClassifierError::Transport(e.to_string()))?;
        parse_alignment(&bytes)
            .map(|alignment| Scored { alignment })
            .ok_or(ClassifierError::Unparseable)
    }
}

impl ClassifierProvider for JevClassifier {
    fn score<'a>(&'a self, req: ScoreRequest<'a>) -> ScoreFuture<'a> {
        // Boxes the inherent async fn to satisfy the object-safe trait.
        Box::pin(self.score_impl(req))
    }
}

/// Tolerant alignment extraction (D28): accepts the response dialects we
/// have observed across vendor iterations —
///
/// ```json
/// {"alignment": 0.87}
/// {"score": "0.31"}
/// {"result": {"alignment": 0.12, "model": "jev-1"}}
/// {"alignment_score": 0.5}
/// ```
///
/// Numbers or numeric strings, top-level or one `result`/`data` level
/// deep, clamped to 0..=1. Anything else → `None` (fail-open upstream).
pub fn parse_alignment(bytes: &[u8]) -> Option<f64> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    // Candidate keys in priority order; nested one level under common
    // wrapper names.
    const KEYS: [&str; 3] = ["alignment", "alignment_score", "score"];
    let containers: Vec<&Value> = {
        let mut v = vec![&value];
        for wrapper in ["result", "data"] {
            if let Some(inner) = value.get(wrapper) {
                v.push(inner);
            }
        }
        v
    };
    for container in containers {
        for key in KEYS {
            if let Some(number) = container.get(key).and_then(coerce_number) {
                return Some(number.clamp(0.0, 1.0));
            }
        }
    }
    None
}

/// Coerces a JSON value into f64: native numbers, or numeric strings
/// (vendors serialize floats as strings more often than you would think).
fn coerce_number(raw: &Value) -> Option<f64> {
    match raw {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_dialect() {
        assert_eq!(parse_alignment(br#"{"alignment": 0.87}"#), Some(0.87));
    }

    #[test]
    fn parses_numeric_string_and_alternate_keys() {
        assert_eq!(parse_alignment(br#"{"score": "0.31"}"#), Some(0.31));
        assert_eq!(parse_alignment(br#"{"alignment_score": 0.5}"#), Some(0.5));
    }

    #[test]
    fn parses_nested_result_wrapper() {
        let body = br#"{"result": {"alignment": 0.12, "model": "jev-1"}}"#;
        assert_eq!(parse_alignment(body), Some(0.12));
        let body = br#"{"data": {"score": "0.99"}}"#;
        assert_eq!(parse_alignment(body), Some(0.99));
    }

    #[test]
    fn clamps_out_of_range_values() {
        assert_eq!(parse_alignment(br#"{"alignment": 7.5}"#), Some(1.0));
        assert_eq!(parse_alignment(br#"{"alignment": -3}"#), Some(0.0));
    }

    #[test]
    fn garbage_fails_open_to_none() {
        assert_eq!(parse_alignment(b"<html>boom</html>"), None);
        assert_eq!(parse_alignment(br#"{"error": "nope"}"#), None);
        assert_eq!(parse_alignment(br#"{"alignment": "high"}"#), None);
        assert_eq!(parse_alignment(b""), None);
    }

    #[test]
    fn top_level_key_beats_nested() {
        let body = br#"{"alignment": 0.4, "result": {"alignment": 0.9}}"#;
        assert_eq!(parse_alignment(body), Some(0.4));
    }
}
