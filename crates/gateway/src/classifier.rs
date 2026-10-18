//! Async, sampled, off-hot-path purpose classification (D28, M4).
//!
//! Flow: the chat path (AFTER the response is served and metered) hands a
//! [`ScoreContext`] to [`spawn_scoring`]. The spawned task:
//!
//! 1. loads the org's classifier config — no row ⇒ nothing happens
//!    (dormant-by-default, D28);
//! 2. checks the sampling budget (N scores/hour/org — scoring every request
//!    would burn the org's classifier credits for little signal);
//! 3. decrypts the sealed API key per use (never cached — rule 2 posture);
//! 4. calls the org's provider through the raw-HTTP Jev adapter
//!    (version-pinned, tolerant parsing, 10 s budget);
//! 5. stores the SCORE (`classifier_scores`) — never the prompt text;
//! 6. below the alert threshold ⇒ review-queue flag (Alert mode);
//!    additionally, live auto-block + below the block threshold ⇒ the key
//!    is SUSPENDED pending review (one-click unblock; never auto-ban).
//!
//! Every failure mode fails OPEN: a down/slow/deranged classifier costs a
//! log line, never a request.

use serde_json::Value;
use serde_json::json;
use tributary_common::db::new_id;
use tributary_common::time::{Period, period_start_ms};
use tributary_crypto::row_ad;
use zeroize::Zeroizing;

use tributary_control::classifier::{ClassifierProvider, JevClassifier, ScoreRequest};
use tributary_control::db::{self as cdb};
use tributary_control::security::{self, Detection, DetectionKind};

use crate::state::AppState;

/// What one scoring attempt needs to know about the request it scores.
/// The prompt TEXT is the only sensitive field — it is sent to the
/// classifier and dropped; nothing here is ever persisted.
pub struct ScoreContext {
    pub org_id: String,
    pub pool_id: String,
    pub pool_key_id: String,
    pub key_prefix: String,
    pub user_id: Option<String>,
    pub model: String,
    /// Extracted prompt material (bounded by [`extract_prompt`]).
    pub prompt_text: String,
}

/// How much prompt text we ever ship to a classifier (chars ≈ bytes for
/// the ASCII-dominant case; a hard cap bounds cost and exfiltration
/// surface for BYOK keys).
pub const MAX_PROMPT_CHARS: usize = 8192;

/// Extracts the textual content of an OpenAI-style chat body: the
/// `content` of every message, joined with newlines, hard-capped.
///
/// Never logged, never stored — it exists only inside the scoring task.
pub fn extract_prompt(body: &Value) -> String {
    let mut text = String::new();
    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        for message in messages {
            if let Some(content) = message.get("content").and_then(Value::as_str) {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(content);
                if text.len() >= MAX_PROMPT_CHARS {
                    text.truncate(MAX_PROMPT_CHARS);
                    break;
                }
            }
        }
    }
    text
}

/// Spawns the scoring task. Fire-and-forget by design: the response is
/// already with the client by the time the caller invokes this.
pub fn spawn_scoring(state: AppState, ctx: ScoreContext) {
    tokio::spawn(async move {
        if let Err(err) = run_scoring(&state, &ctx).await {
            // Fail-open: log and move on. The request that triggered this
            // was served long ago.
            tracing::warn!(
                "classifier scoring skipped (org={} key={}): {err:#}",
                ctx.org_id,
                ctx.key_prefix
            );
        }
    });
}

/// One full scoring attempt (also directly callable from tests).
async fn run_scoring(state: &AppState, ctx: &ScoreContext) -> anyhow::Result<()> {
    // 1) Dormancy gate: no config row, no classification (D28).
    let Some(config) = cdb::classifier_config_by_org(&state.db, &ctx.org_id).await? else {
        return Ok(());
    };

    // 2) Sampling budget: at most N scores per org per hour. Approximate
    // (check-then-insert, not a lock) — off-by-one under concurrency is
    // acceptable for a sampling knob.
    let now = tributary_common::db::now_ms();
    let hour_start = period_start_ms(Period::Hour, now);
    let sampled = cdb::classifier_samples_this_hour(&state.db, &ctx.org_id, hour_start).await?;
    if sampled >= config.sampling_per_hour {
        return Ok(());
    }

    // 3) Per-use decrypt under Zeroizing (rule 2 posture; AD binds the
    //    blob to the org's config row — copy-paste rebinding fails).
    let sealed = tributary_crypto::SealedEnvelope {
        ciphertext: config.ciphertext.clone(),
        dek_wrapped: config.dek_wrapped.clone(),
    };
    let plaintext = tributary_crypto::open(
        &state.config.master_key,
        &sealed,
        &row_ad("classifier_config", &config.org_id),
    )?;
    let api_key = Zeroizing::new(
        String::from_utf8(plaintext.to_vec())
            .map_err(|_| anyhow::anyhow!("sealed classifier key is not valid UTF-8"))?,
    );

    // 4) Call the provider (raw HTTP, version-pinned path).
    let provider = JevClassifier::new(state.http.clone(), &config.base_url, &config.model, api_key);
    let scored = provider
        .score(ScoreRequest {
            model: &config.model,
            purpose: &config.purpose_statement,
            text: &ctx.prompt_text,
        })
        .await
        .map_err(|e| anyhow::anyhow!("provider call failed (fail-open): {e:#}"))?;

    let score_bp = (scored.alignment * 10_000.0).round() as i64;
    let would_block = score_bp < config.block_threshold_bp;

    // 5) Store the score (NEVER the prompt).
    cdb::insert_classifier_score(
        &state.db,
        &cdb::NewClassifierScore {
            id: &new_id(),
            org_id: &ctx.org_id,
            pool_key_id: Some(&ctx.pool_key_id),
            user_id: ctx.user_id.as_deref(),
            model: &ctx.model,
            score_bp,
            would_block,
            ts: now,
        },
    )
    .await?;

    // Sanitized detail — score, model, masked prefix, ip-less. The
    // reviewer sees WHAT scored badly, never the prompt content.
    let detail = json!({
        "score": scored.alignment,
        "model": ctx.model,
        "key_prefix": ctx.key_prefix,
        "alert_threshold": config.alert_threshold_bp as f64 / 10_000.0,
    });

    // 6a) Alert-mode flag.
    if score_bp < config.alert_threshold_bp {
        security::detect(
            &state.db,
            state.config.smtp.as_ref(),
            &Detection {
                org_id: &ctx.org_id,
                pool_id: Some(&ctx.pool_id),
                pool_key_id: Some(&ctx.pool_key_id),
                user_id: ctx.user_id.as_deref(),
                kind: DetectionKind::ClassifierFlag,
                severity: None,
                score_bp: Some(score_bp),
                detail: detail.clone(),
                email: true,
            },
        )
        .await;
    }

    // 6b) Live auto-block: suspend the KEY pending review (D28: shadow
    // first; one-click unblock; never auto-ban the member).
    if config.mode == "auto_block" && config.auto_block_live && would_block && ctx.user_id.is_some()
    {
        cdb::suspend_pool_key(&state.db, &ctx.pool_key_id).await?;
        security::detect(
            &state.db,
            state.config.smtp.as_ref(),
            &Detection {
                org_id: &ctx.org_id,
                pool_id: Some(&ctx.pool_id),
                pool_key_id: Some(&ctx.pool_key_id),
                user_id: ctx.user_id.as_deref(),
                kind: DetectionKind::ClassifierAutoBlock,
                severity: None,
                score_bp: Some(score_bp),
                detail: json!({
                    "score": scored.alignment,
                    "model": ctx.model,
                    "key_prefix": ctx.key_prefix,
                    "action": "key suspended pending review (one-click unblock)",
                }),
                email: true,
            },
        )
        .await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prompt_extraction_joins_and_caps() {
        let body = json!({
            "model": "m",
            "messages": [
                {"role": "system", "content": "be brief"},
                {"role": "user", "content": "hello there"},
            ],
        });
        assert_eq!(extract_prompt(&body), "be brief\nhello there");
        // Non-string content (image parts) contributes nothing.
        let body = json!({"messages": [{"role": "user", "content": {"type": "image_url"}}]});
        assert_eq!(extract_prompt(&body), "");
    }

    #[test]
    fn prompt_extraction_hard_caps() {
        let big = "x".repeat(MAX_PROMPT_CHARS + 500);
        let body = json!({"messages": [{"role": "user", "content": big}]});
        assert_eq!(extract_prompt(&body).len(), MAX_PROMPT_CHARS);
    }
}
