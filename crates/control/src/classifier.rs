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
//!
//! **Dialects (D28 + director research 2026-10-21):** the org picks which
//! contract its endpoint speaks. `"jev"` is TypeSafe AI's real hosted
//! contract (`POST {base}/v1/systemone`, noul-typed question, score at
//! `answers.alignment.noul`); `"custom"` is the generic 0..1 scorer shape
//! this crate originally shipped. Adding the real dialect changed the
//! adapter ONLY — the version pin did exactly what it was designed for.

use std::future::Future;
use std::pin::Pin;

use serde_json::Value;
use zeroize::Zeroizing;

/// The generic-dialect endpoint appended to `base_url` (see [`Dialect`]).
/// "Version-pinned" means: when a vendor ships a new shape, we add a NEW
/// constant + dialect — we never silently follow a redirect or floating
/// "latest" path (D28 churn defense).
pub const ALIGN_ENDPOINT: &str = "/v1/align";

/// The TypeSafe/Jev dialect endpoint (director research 2026-10-21).
pub const SYSTEMONE_ENDPOINT: &str = "/v1/systemone";

/// Budget for one classification call (async, off the hot path, but still
/// bounded — a hung vendor must not leak tasks forever).
pub const SCORE_TIMEOUT_SECS: u64 = 10;

/// Which contract the org's endpoint speaks. Stored per org in
/// `classifier_config.dialect` (default `"custom"` — pre-dialect rows keep
/// working unchanged).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// TypeSafe AI Jev, hosted: `POST {base}/v1/systemone` with a
    /// noul-typed alignment question; score at `answers.alignment.noul`.
    Jev,
    /// The generic 0..1 scorer this crate shipped originally
    /// (`POST {base}/v1/align`). Kept for any simple scoring endpoint an
    /// org may point at.
    Custom,
}

impl Dialect {
    /// Parses the wire/DB form; anything unknown falls back to `Custom`
    /// (existing rows predate the column and mean the generic shape).
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_lowercase().as_str() {
            "jev" => Dialect::Jev,
            _ => Dialect::Custom,
        }
    }

    /// The wire/DB form (also the value the dashboard selects).
    pub fn as_str(self) -> &'static str {
        match self {
            Dialect::Jev => "jev",
            Dialect::Custom => "custom",
        }
    }
}

/// The default base URL for the hosted Jev dialect (director: hosted only,
/// no self-host variant to support). Overridable so tests (and the local
/// mock) can point elsewhere.
pub const JEV_DEFAULT_BASE_URL: &str = "https://api.typesafe.ai";

/// Default model for the Jev dialect — floats with the vendor; orgs can
/// pin an exact version for reproducibility.
pub const JEV_DEFAULT_MODEL: &str = "jev-latest";

/// Jev's `state.message` cap (documented ~4k chars). Tighter than our
/// 8192-char extraction cap — the adapter truncates, cost stays bounded.
pub const JEV_MAX_MESSAGE_CHARS: usize = 4096;

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
    dialect: Dialect,
    api_key: Zeroizing<String>,
}

impl JevClassifier {
    /// Builds a provider bound to one org's config snapshot. `dialect`
    /// picks the wire contract (see [`Dialect`]); the key lives in
    /// `Zeroizing` and is dropped with the adapter (never cached).
    pub fn new(
        http: reqwest::Client,
        base_url: &str,
        model: &str,
        dialect: Dialect,
        api_key: Zeroizing<String>,
    ) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            dialect,
            api_key,
        }
    }

    /// Plain async implementation — the trait adapter wraps it in a boxed
    /// future (kept separate so unit tests can await it directly).
    async fn score_impl(&self, req: ScoreRequest<'_>) -> Result<Scored, ClassifierError> {
        let (url, payload) = match self.dialect {
            Dialect::Jev => (SYSTEMONE_ENDPOINT, systemone_body(&self.model, req)),
            Dialect::Custom => (ALIGN_ENDPOINT, custom_body(&self.model, req)),
        };
        let response = self
            .http
            .post(format!("{}{}", self.base_url, url))
            .bearer_auth(self.api_key.as_str())
            .timeout(std::time::Duration::from_secs(SCORE_TIMEOUT_SECS))
            .json(&payload)
            .send()
            .await
            .map_err(|e| ClassifierError::Transport(e.to_string()))?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            // The body may carry a vendor error message — useful in logs,
            // never surfaced to members. Distinct codes stay distinct
            // (401 auth, 422 shape, 429/529 capacity) so the caller can
            // back off on 429/529 without punishing a wrong key.
            return Err(ClassifierError::Status(status));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| ClassifierError::Transport(e.to_string()))?;
        let alignment = match self.dialect {
            Dialect::Jev => parse_systemone(&bytes),
            Dialect::Custom => parse_alignment(&bytes),
        }
        .ok_or(ClassifierError::Unparseable)?;
        Ok(Scored { alignment })
    }
}

/// Builds the TypeSafe/Jev systemone request body from the documented
/// example shape (director research 2026-10-21):
/// `state.message` carries the request text capped at
/// [`JEV_MAX_MESSAGE_CHARS`]; the org's purpose is embedded verbatim in
/// the noul question's instructions.
fn systemone_body(model: &str, req: ScoreRequest<'_>) -> Value {
    // Owned String either way — truncation rebuilds on char boundaries
    // (slicing UTF-8 by byte index could split a character and hand the
    // vendor invalid text), and the non-truncating clone is bounded by
    // the 8192-char extraction cap upstream.
    let message: String = if req.text.chars().count() > JEV_MAX_MESSAGE_CHARS {
        req.text.chars().take(JEV_MAX_MESSAGE_CHARS).collect()
    } else {
        req.text.to_string()
    };
    serde_json::json!({
        "model": model,
        "state": { "message": message },
        "questions": {
            "alignment": {
                "type": "noul",
                "instructions": format!(
                    "Does this AI request serve the org's purpose: {}?",
                    req.purpose
                ),
            }
        },
    })
}

/// Builds the generic `/v1/align` body (the shape this crate shipped
/// originally — kept as dialect `"custom"` for simple scorer endpoints).
fn custom_body(model: &str, req: ScoreRequest<'_>) -> Value {
    serde_json::json!({
        "model": model,
        "purpose": req.purpose,
        "text": req.text,
    })
}

/// Parses the Jev systemone response per the documented shape: the score
/// lives at `answers.alignment.noul`, as a number or numeric string,
/// clamped to 0..=1. Tolerant of extra sibling fields; anything else →
/// `None` (fail-open upstream, like every parse here).
pub fn parse_systemone(bytes: &[u8]) -> Option<f64> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    value
        .get("answers")?
        .get("alignment")?
        .get("noul")
        .and_then(coerce_number)
        .map(|n| n.clamp(0.0, 1.0))
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
    fn dialect_parse_round_trips_and_defaults_to_custom() {
        assert_eq!(Dialect::parse("jev"), Dialect::Jev);
        assert_eq!(Dialect::parse("Jev"), Dialect::Jev);
        assert_eq!(Dialect::parse("custom"), Dialect::Custom);
        // Unknown (or pre-dialect garbage) means the generic shape.
        assert_eq!(Dialect::parse("nope"), Dialect::Custom);
        assert_eq!(Dialect::parse(""), Dialect::Custom);
        assert_eq!(Dialect::Jev.as_str(), "jev");
        assert_eq!(Dialect::Custom.as_str(), "custom");
    }

    #[test]
    fn systemone_body_matches_documented_shape() {
        let body = systemone_body(
            "jev-latest",
            ScoreRequest {
                model: "jev-latest",
                purpose: "coding only",
                text: "OFF-PURPOSE x",
            },
        );
        assert_eq!(body["model"], "jev-latest");
        assert_eq!(body["state"]["message"], "OFF-PURPOSE x");
        assert_eq!(body["questions"]["alignment"]["type"], "noul");
        assert_eq!(
            body["questions"]["alignment"]["instructions"],
            "Does this AI request serve the org's purpose: coding only?"
        );
    }

    #[test]
    fn systemone_body_caps_message_at_char_boundary() {
        let text = "à".repeat(JEV_MAX_MESSAGE_CHARS + 10); // 2-byte chars
        let body = systemone_body(
            "jev-latest",
            ScoreRequest {
                model: "jev-latest",
                purpose: "p",
                text: &text,
            },
        );
        let shipped: String = body["state"]["message"].as_str().unwrap().to_string();
        assert_eq!(shipped.chars().count(), JEV_MAX_MESSAGE_CHARS);
        // Every char intact (no broken UTF-8 from a byte-index slice).
        assert!(shipped.chars().all(|c| c == 'à'));
    }

    #[test]
    fn parses_systemone_documented_shape() {
        let body = br#"{"answers": {"alignment": {"noul": 0.87}}}"#;
        assert_eq!(parse_systemone(body), Some(0.87));
        // Numeric string form + extra sibling fields.
        let body = br#"{"answers": {"alignment": {"noul": "0.31", "why": "x"}, "other": 1}}"#;
        assert_eq!(parse_systemone(body), Some(0.31));
        // Out-of-range clamps like the generic parser.
        assert_eq!(
            parse_systemone(br#"{"answers":{"alignment":{"noul":7.5}}}"#),
            Some(1.0)
        );
        // Wrong paths fail open.
        assert_eq!(parse_systemone(br#"{"alignment": 0.9}"#), None);
        assert_eq!(parse_systemone(br#"{"answers":{"noul":0.9}}"#), None);
        assert_eq!(parse_systemone(b""), None);
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
