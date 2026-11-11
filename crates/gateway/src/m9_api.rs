//! M9 control-plane endpoints (depositor mode, D43): deposits, bucket
//! positions, settle — the endpoints the doctrine suite's header names.
//!
//! WHY THESE LIVE IN THE GATEWAY CRATE (not control): the deposit path
//! posts `deposit.capacity` through `SqlLedger` and the settle path calls
//! `settle::split_settlement` + the burn-batching read — all ledger
//! surface the gateway already depends on (Session A wired that edge for
//! the single `Bucket` type). Control stays ledger-free; no crate cycle,
//! and no second SQL truth for the same rows.
//!
//! Auth is the control plane's own (`AuthedAdmin` over `ControlState`)
//! — the fragment is merged into the full app by `build_full_app`, which
//! is exactly how the integration tests assemble it too.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use serde::Deserialize;
use serde::Serialize;
use tributary_control::ControlState;
use tributary_control::auth::AuthedAdmin;
use tributary_control::db::PoolRow;
use tributary_control::error::{ApiResult, ControlError};
use tributary_crypto::{row_ad, seal};
use tributary_ledger::chart::{AccountCode, OwnerKind};
use tributary_ledger::money::MicroUsd;
use tributary_ledger::settle::{Bucket, BurnRecord, split_settlement};
use tributary_ledger::{Ledger, PostingRequest, SqlLedger, TemplateCode};
use zeroize::Zeroizing;

use crate::db::{new_id, now_ms};

/// The M9 router fragment (control-state based, ready to merge).
pub fn router(state: ControlState) -> Router {
    Router::new()
        .route("/api/v1/pools/{id}/deposits", post(create_deposit))
        .route(
            "/api/v1/pools/{id}/bucket-positions",
            get(list_bucket_positions),
        )
        .route("/api/v1/pools/{id}/settle", post(settle_pool))
        .with_state(state)
}

/// Loads a pool scoped to the admin (existence + org scope — the same
/// shape as control's private `load_scoped_pool`, reimplemented here
/// only because that helper is crate-private; six lines of glue).
async fn scoped_pool(
    state: &ControlState,
    admin: &AuthedAdmin,
    pool_id: &str,
) -> ApiResult<PoolRow> {
    let pool = tributary_control::db::pool_by_id(&state.db, pool_id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    admin.ensure_org_scope(&pool.org_id)?;
    Ok(pool)
}

/// Depositor mode is a ROOT-pool feature: both depositor endpoints and
/// the settle endpoint REFUSE org pools (4xx) — the suite pins that
/// refusal (invariant 9).
fn require_root(pool: &PoolRow) -> ApiResult<()> {
    if pool.kind == "root" {
        Ok(())
    } else {
        Err(ControlError::BadRequest(
            "depositor mode is a root-pool feature; this pool is an org (V1) pool".into(),
        ))
    }
}

/// POST /api/v1/pools/{id}/deposits body. `key` is plaintext ONCE here
/// (rule 2: envelope-encrypted at rest before the row lands).
#[derive(Deserialize)]
pub struct DepositRequest {
    pub provider: String,
    pub key: String,
    pub base_url: String,
    /// Declared model families — data, never code (families decide which
    /// models this key may serve; hyphen-prefixes count).
    pub model_families: Vec<String>,
    /// Integer micro-USD contributed by this deposit.
    pub contributed_micros: i64,
    /// Link this key to an EXISTING depositor (one depositor, many
    /// buckets); omitted ⇒ a fresh depositor is minted.
    pub depositor_id: Option<String>,
}

/// The 201 response — exactly the contract the suite's header names.
#[derive(Serialize)]
pub struct DepositResponse {
    pub depositor_id: String,
    pub provider_key_id: String,
    /// Masked display form (first-5…last-4 ONLY — secrets convention).
    pub key_prefix: String,
}

/// The per-bucket position row served by GET bucket-positions — the
/// off-chain books (D43).
#[derive(Serialize)]
pub struct BucketPositionResponse {
    pub depositor_id: String,
    pub provider: String,
    pub model_family: String,
    pub contributed_micros: i64,
    pub burned_micros: i64,
    pub earned_micros: i64,
}

/// POST /api/v1/pools/{id}/settle body.
#[derive(Deserialize)]
pub struct SettleRequest {
    pub from_ms: i64,
    pub to_ms: i64,
    /// Operator fee in basis points — rule 8's 5–10% band is enforced
    /// here (the pure split stays total; the SURFACE carries doctrine).
    pub fee_bps: i64,
}

/// The settle response — the contract the suite's header names.
#[derive(Serialize)]
pub struct SettleResponse {
    pub distributable_micros: i64,
    pub fee_micros: i64,
    pub undistributed_micros: i64,
    pub payouts: Vec<SettlePayout>,
}

/// One payout row: depositor × bucket × amount (M11 Merkle leaves).
#[derive(Serialize)]
pub struct SettlePayout {
    pub depositor_id: String,
    pub provider: String,
    pub model_family: String,
    pub amount_micros: i64,
}

/// Creates a deposit: seal the key (rule 2), insert the provider-key row
/// carrying `depositor_id` + declared families, open the per-bucket
/// position rows (their sum must equal this deposit's contribution), and
/// post `deposit.capacity` through the books (claims.contributed grows —
/// the single contributed-value source, migration 0015's liability).
pub async fn create_deposit(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    headers: axum::http::HeaderMap,
    Path(pool_id): Path<String>,
    Json(body): Json<DepositRequest>,
) -> ApiResult<(StatusCode, Json<DepositResponse>)> {
    let pool = scoped_pool(&state, &admin, &pool_id).await?;
    require_root(&pool)?;
    if pool.status != "active" {
        return Err(ControlError::Conflict("This pool is disabled.".into()));
    }

    // Plaintext lives exactly once, inside Zeroizing (secrets convention).
    let raw_key = Zeroizing::new(body.key);
    if raw_key.len() < 8 {
        return Err(ControlError::BadRequest(
            "key looks too short to be real".into(),
        ));
    }
    if !(body.base_url.starts_with("http://") || body.base_url.starts_with("https://")) {
        return Err(ControlError::BadRequest("base_url must be http(s)".into()));
    }
    if body.contributed_micros <= 0 {
        return Err(ControlError::BadRequest(
            "contributed_micros must be a positive integer (micro-USD)".into(),
        ));
    }
    // Families: nonempty, deduplicated preserving first declaration (the
    // order IS deterministic — remainder micros land on the first).
    let mut families: Vec<String> = Vec::new();
    for family in &body.model_families {
        let trimmed = family.trim();
        if trimmed.is_empty() {
            return Err(ControlError::BadRequest(
                "model_families must name at least one non-empty family".into(),
            ));
        }
        if !families.iter().any(|f| f == trimmed) {
            families.push(trimmed.to_string());
        }
    }
    if families.is_empty() {
        return Err(ControlError::BadRequest(
            "model_families must name at least one family".into(),
        ));
    }
    let provider = body.provider.trim();
    if provider.is_empty() {
        return Err(ControlError::BadRequest("provider must be named".into()));
    }

    // Mint the identity set: a fresh depositor when none was linked.
    let depositor_id = body.depositor_id.clone().unwrap_or_else(new_id);
    let provider_key_id = new_id();

    // Seal BEFORE any row exists; the minted id doubles as the AD binding
    // (the V1 provider-key path established this order).
    let sealed: tributary_crypto::SealedEnvelope = seal(
        &state.master,
        raw_key.as_bytes(),
        &row_ad("provider_keys", &provider_key_id),
    )
    .map_err(ControlError::Internal)?;
    let key_prefix = display_prefix(&raw_key);

    // The provider-key row, depositor edition: the two migration-0016
    // columns carry WHO contributes and WHICH families the key declares.
    // (Dedicated INSERT, not a shared helper: the V1 path stays
    // byte-identical — invariant 9/10.)
    let families_json =
        serde_json::to_string(&families).map_err(|e| ControlError::Internal(e.into()))?;
    let insert_key = state.db.sql(
        "INSERT INTO provider_keys \
         (id, pool_id, provider, base_url, ciphertext, dek_wrapped, key_prefix, \
          health, last_validated_at, status, created_at, depositor_id, model_families) \
         VALUES (?, ?, ?, ?, ?, ?, ?, 'unknown', NULL, 'active', ?, ?, ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(insert_key.as_str()))
        .bind(&provider_key_id)
        .bind(&pool.id)
        .bind(provider)
        .bind(&body.base_url)
        .bind(&sealed.ciphertext)
        .bind(&sealed.dek_wrapped)
        .bind(&key_prefix)
        .bind(now_ms())
        .bind(&depositor_id)
        .bind(&families_json)
        .execute(state.db.pool())
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;

    // Per-bucket positions: split the contribution across the declared
    // families (floor micros, remainder to the earliest-declared), so
    // Σ positions == this deposit's claims.contributed credit — the
    // invariant the suite pins. Upsert: a repeat deposit into the same
    // bucket ADDS to the position (books stay additive).
    let per_family = body.contributed_micros / families.len() as i64;
    let mut remainder = body.contributed_micros - per_family * families.len() as i64;
    for family in &families {
        let share = per_family + remainder.min(1);
        if remainder > 0 {
            remainder -= 1;
        }
        let insert_position = state.db.sql(
            "INSERT INTO depositor_bucket_positions \
             (id, pool_id, depositor_id, provider, model_family, \
              contributed_micros, burned_micros, earned_micros, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 0, 0, ?) \
             ON CONFLICT (pool_id, depositor_id, provider, model_family) DO UPDATE \
             SET contributed_micros = depositor_bucket_positions.contributed_micros \
                 + excluded.contributed_micros, \
                 updated_at = excluded.updated_at",
        );
        sqlx::query(sqlx::AssertSqlSafe(insert_position.as_str()))
            .bind(new_id())
            .bind(&pool.id)
            .bind(&depositor_id)
            .bind(provider)
            .bind(family)
            .bind(share)
            .bind(now_ms())
            .execute(state.db.pool())
            .await
            .map_err(|e| ControlError::Internal(e.into()))?;
    }

    // The books: open the depositor's capacity/claims accounts (idempotent)
    // and post deposit.capacity — claims.contributed grows by exactly the
    // contribution; the positions above must sum back to it.
    let ledger = SqlLedger::new(state.db.clone());
    for code in [
        AccountCode::CapacityContributed,
        AccountCode::ClaimsContributed,
    ] {
        ledger
            .open_account(code, OwnerKind::Depositor, &depositor_id)
            .await
            .map_err(|e| ControlError::Internal(e.into()))?;
    }
    ledger
        .post(
            PostingRequest::new(
                TemplateCode::DepositCapacity,
                MicroUsd::new(body.contributed_micros),
            )
            .depositor(&depositor_id)
            .reference(&format!("m9.deposit.{provider_key_id}")),
        )
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;

    tributary_control::audit::record(
        &state.db,
        tributary_control::audit::AuditEntry {
            org_id: Some(&pool.org_id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "depositor.deposit",
            target_kind: tributary_control::audit::TargetKind::ProviderKey,
            target_id: &provider_key_id,
            before: None,
            // Masked view only — no key material, no ciphertext (rule 2).
            after: Some(serde_json::json!({
                "pool_id": pool.id,
                "depositor_id": depositor_id,
                "provider": provider,
                "model_families": families,
                "contributed_micros": body.contributed_micros,
                "key_prefix": key_prefix,
            })),
            ip: Some(&tributary_common::net::client_ip(&headers)),
        },
    )
    .await;

    Ok((
        StatusCode::CREATED,
        Json(DepositResponse {
            depositor_id,
            provider_key_id,
            key_prefix,
        }),
    ))
}

/// Lists the pool's per-depositor × per-bucket positions — the off-chain
/// books (D43). Order is the deterministic read order (depositor,
/// provider, family).
pub async fn list_bucket_positions(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path(pool_id): Path<String>,
) -> ApiResult<Json<Vec<BucketPositionResponse>>> {
    let pool = scoped_pool(&state, &admin, &pool_id).await?;
    require_root(&pool)?;
    let positions = tributary_ledger::settle::bucket_positions(&state.db, &pool.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(
        positions
            .iter()
            .map(|p| BucketPositionResponse {
                depositor_id: p.depositor_id.clone(),
                provider: p.bucket.provider.clone(),
                model_family: p.bucket.model_family.clone(),
                contributed_micros: p.contributed_micros.micros(),
                burned_micros: p.burned_micros.micros(),
                earned_micros: p.earned_micros.micros(),
            })
            .collect(),
    ))
}

/// Settles a period: batch the window's attributed usage rows into burns,
/// derive each bucket's distributable (Σ row cost), split it with the D43
/// doctrine (fee on burned buckets; earnings follow the burn, per bucket;
/// burnless distributable returned), credit the payouts onto the bucket
/// positions, and return the split.
///
/// M9 SCOPE (director-accepted asymmetry): positions update directly; the
/// endpoint does NOT post settle.fee/settle.distribute through the books
/// (the ledger side of settle is pinned by the ledger suite via the
/// public trait — Session C probes this seam).
pub async fn settle_pool(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path(pool_id): Path<String>,
    Json(body): Json<SettleRequest>,
) -> ApiResult<Json<SettleResponse>> {
    let pool = scoped_pool(&state, &admin, &pool_id).await?;
    require_root(&pool)?;
    if body.from_ms >= body.to_ms {
        return Err(ControlError::BadRequest(
            "from_ms must be strictly before to_ms".into(),
        ));
    }
    // Rule 8: the operator fee lives in the 5–10% band (target 8%, D21).
    // Changing the band is a user-approved D-log matter, not a payload.
    if !(500..=1000).contains(&body.fee_bps) {
        return Err(ControlError::BadRequest(
            "fee_bps must stay inside the 5-10% band (500..=1000) — rule 8".into(),
        ));
    }

    // The batch: every attributed row in the window (V1 rows never carry
    // attribution; a root pool's rows always do — the suite hunts gaps).
    let attributions = crate::depositor_mode::attribute_period_burns(
        &state.db,
        &pool.id,
        body.from_ms,
        body.to_ms,
    )
    .await
    .map_err(|e| ControlError::Internal(e.into()))?;

    // Distributable per bucket = Σ cost of that bucket's attributed rows
    // (a bucket with rows but zero total still burned nothing payable —
    // the split treats burn_total <= 0 as burnless and returns it).
    let mut distributable: Vec<(Bucket, i64)> = Vec::new();
    let mut burns: Vec<BurnRecord> = Vec::new();
    for attribution in &attributions {
        match distributable
            .iter_mut()
            .find(|(bucket, _)| bucket == &attribution.bucket)
        {
            Some((_, total)) => *total += attribution.burn_micros.micros(),
            None => {
                distributable.push((attribution.bucket.clone(), attribution.burn_micros.micros()))
            }
        }
        burns.push(BurnRecord {
            depositor_id: attribution.depositor_id.clone(),
            bucket: attribution.bucket.clone(),
            burn_micros: attribution.burn_micros,
        });
    }

    let split = split_settlement(
        &distributable
            .iter()
            .map(|(bucket, micros)| (bucket.clone(), MicroUsd::new(*micros)))
            .collect::<Vec<_>>(),
        &burns,
        body.fee_bps,
    );

    // Credit the payouts onto the bucket positions (earned grows by the
    // settled amount; contributed/burned are untouched here).
    for payout in &split.payouts {
        let update = state.db.sql(
            "UPDATE depositor_bucket_positions \
             SET earned_micros = earned_micros + ?, updated_at = ? \
             WHERE pool_id = ? AND depositor_id = ? AND provider = ? AND model_family = ?",
        );
        sqlx::query(sqlx::AssertSqlSafe(update.as_str()))
            .bind(payout.amount.micros())
            .bind(now_ms())
            .bind(&pool.id)
            .bind(&payout.depositor_id)
            .bind(&payout.bucket.provider)
            .bind(&payout.bucket.model_family)
            .execute(state.db.pool())
            .await
            .map_err(|e| ControlError::Internal(e.into()))?;
    }

    let distributable_total: i64 = distributable.iter().map(|(_, micros)| *micros).sum();
    Ok(Json(SettleResponse {
        distributable_micros: distributable_total,
        fee_micros: split.fee.micros(),
        undistributed_micros: split.undistributed.micros(),
        payouts: split
            .payouts
            .iter()
            .map(|p| SettlePayout {
                depositor_id: p.depositor_id.clone(),
                provider: p.bucket.provider.clone(),
                model_family: p.bucket.model_family.clone(),
                amount_micros: p.amount.micros(),
            })
            .collect(),
    }))
}

/// Masks a raw key into the display form (first-5…last-4, secrets rule).
/// Same shape as control's private helper — reimplemented for the same
/// layering reason as `scoped_pool` (the helper is crate-private there).
fn display_prefix(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    if chars.len() < 10 {
        return format!("{}…", &raw[..chars.len().saturating_sub(2).max(1)]);
    }
    format!(
        "{}…{}",
        chars[..5].iter().collect::<String>(),
        chars[chars.len() - 4..].iter().collect::<String>()
    )
}
