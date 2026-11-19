//! M9 control-plane endpoints (depositor mode, D43): deposits, bucket
//! positions, settle — the endpoints the doctrine suite's header names.
//!
//! WHY THESE LIVE IN THE GATEWAY CRATE (not control): the deposit path
//! posts `deposit.capacity` and the settle path posts the full M9-D
//! settlement batch (`capacity.consume` → `settle.fee` →
//! `settle.distribute`) through `SqlLedger`, plus the positions read —
//! all ledger surface the gateway already depends on (Session A wired
//! that edge for the single `Bucket` type). Control stays ledger-free;
//! no crate cycle, and no second SQL truth for the same rows.
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
use sqlx::Row;
use std::collections::HashMap;
use tributary_control::ControlState;
use tributary_control::auth::AuthedAdmin;
use tributary_control::db::PoolRow;
use tributary_control::error::{ApiResult, ControlError};
use tributary_crypto::{row_ad, seal};
use tributary_ledger::chart::{AccountCode, OwnerKind, PLATFORM_OWNER_ID};
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

/// The settle response — the contract the suite's header names. Also the
/// payload persisted on the settle-run journal (`response_json`): an
/// identical-window retry deserializes and replays it verbatim (A-048),
/// hence the Deserialize half.
#[derive(Serialize, Deserialize)]
pub struct SettleResponse {
    pub distributable_micros: i64,
    pub fee_micros: i64,
    pub undistributed_micros: i64,
    pub payouts: Vec<SettlePayout>,
}

/// One payout row: depositor × bucket × amount (M11 Merkle leaves).
#[derive(Serialize, Deserialize)]
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

/// A settle-run journal row, as the endpoint reads it back.
struct SettleRun {
    status: String,
    response_json: String,
}

/// Finds the run for an exact (pool, window), if any — A-048's
/// idempotency lookup. SQL-only helper, assertion-free by design.
async fn find_settle_run(
    state: &ControlState,
    pool_id: &str,
    from_ms: i64,
    to_ms: i64,
) -> Result<Option<SettleRun>, ControlError> {
    let sql = state.db.sql(
        "SELECT status, response_json FROM settle_runs \
         WHERE pool_id = ? AND from_ms = ? AND to_ms = ?",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_id)
        .bind(from_ms)
        .bind(to_ms)
        .fetch_optional(state.db.pool())
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(row.map(|row| SettleRun {
        status: row.get("status"),
        response_json: row.get("response_json"),
    }))
}

/// Settles a period (A-048/A-049-fixed, 2026-11-19):
///
/// 1. IDEMPOTENCY — an identical (pool, window) retry replays the FIRST
///    run's stored response verbatim: a client retry after a timed-out
///    settle is a NO-OP on positions AND on the books (director ruling:
///    retry = no-op on both sides). A 'failed' run is a loud tombstone
///    (409) — its window is spent; an adjacent window still settles,
///    because attribution is delta-based, never window-re-read.
/// 2. ATTRIBUTION — per (depositor, bucket) position, exactly the
///    un-attributed slice `burned − settled_burn`: the same burn can
///    never re-batch (A-048), and the slice can never exceed backing
///    because the response-end tracker is clamped at `contributed`
///    (A-047) — absorbed overage never becomes distributable.
/// 3. THE BOOKS (director ruling 2026-11-19: settle posts automatically;
///    positions become true caches) — per depositor, `capacity.consume`
///    converts the attributed burn from a principal claim into the
///    settlement pot (claims.contributed → revenue.usage; founder-
///    approved template), then `settle.fee` takes the operator cut
///    (→ equity.operator, D39) and `settle.distribute` moves each payout
///    to `payable.depositor`. Every posting references the run id — one
///    run = one batch of postings, replayable by audit.
/// 4. POSITIONS — `earned` grows by the payouts and the attributed slice
///    is marked settled via a compare-and-set on `settled_burn`, so a
///    concurrent overlapping settle cannot double-credit; drift between
///    the books and the positions is the drift alarm the house ledger
///    rule already owns.
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

    // (1) Idempotency: the exact window's first run is the only answer.
    if let Some(run) = find_settle_run(&state, &pool.id, body.from_ms, body.to_ms).await? {
        return replay_run(run);
    }

    // (2) Attribution: the un-attributed slice of every position. i128
    // accumulators throughout (A-053 leg 1): hostile row costs saturate
    // i64 at METERING, but settlement arithmetic can never panic on them.
    let positions = tributary_ledger::settle::bucket_positions(&state.db, &pool.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let mut burns: Vec<BurnRecord> = Vec::new();
    let mut distributable_acc: HashMap<Bucket, i128> = HashMap::new();
    let mut attribution_by_depositor: HashMap<String, i64> = HashMap::new();
    for p in &positions {
        let delta = (p.burned_micros.micros() - p.settled_burn_micros).max(0);
        if delta == 0 {
            continue; // nothing un-attributed in this bucket (A-048)
        }
        *distributable_acc.entry(p.bucket.clone()).or_insert(0) += delta as i128;
        *attribution_by_depositor
            .entry(p.depositor_id.clone())
            .or_insert(0) += delta;
        burns.push(BurnRecord {
            depositor_id: p.depositor_id.clone(),
            bucket: p.bucket.clone(),
            burn_micros: MicroUsd::new(delta),
        });
    }
    let mut distributable: Vec<(Bucket, MicroUsd)> = Vec::new();
    let mut distributable_total: i128 = 0;
    for (bucket, micros) in distributable_acc {
        // Checked narrowing — a beyond-i64 bucket total is a hard error,
        // never a wrapped number (A-053 leg 1).
        let bucket_label = format!("{}/{}", bucket.provider, bucket.model_family);
        let narrowed = i64::try_from(micros).map_err(|_| {
            ControlError::Internal(anyhow::anyhow!(
                "distributable overflow for bucket {bucket_label}"
            ))
        })?;
        distributable.push((bucket, MicroUsd::new(narrowed)));
        distributable_total += micros;
    }
    let distributable_total = i64::try_from(distributable_total)
        .map_err(|_| ControlError::Internal(anyhow::anyhow!("distributable total overflow")))?;

    let split = split_settlement(&distributable, &burns, body.fee_bps);
    let response = SettleResponse {
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
    };
    let response_json =
        serde_json::to_string(&response).map_err(|e| ControlError::Internal(e.into()))?;

    // Claim the window: the UNIQUE (pool, window) row is the idempotency
    // lock (A-048). A concurrent identical settle loses this insert and
    // takes the replay/409 path above.
    let run_id = new_id();
    let insert = state.db.sql(
        "INSERT INTO settle_runs \
         (id, pool_id, from_ms, to_ms, fee_bps, distributable_micros, \
          fee_micros, undistributed_micros, response_json, status, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?)",
    );
    let inserted = sqlx::query(sqlx::AssertSqlSafe(insert.as_str()))
        .bind(&run_id)
        .bind(&pool.id)
        .bind(body.from_ms)
        .bind(body.to_ms)
        .bind(body.fee_bps)
        .bind(response.distributable_micros)
        .bind(response.fee_micros)
        .bind(response.undistributed_micros)
        .bind(&response_json)
        .bind(now_ms())
        .execute(state.db.pool())
        .await;
    if inserted.is_err() {
        // Lost the race (or real failure): the run that exists owns the
        // window — replay it or refuse, never double-settle.
        if let Some(run) = find_settle_run(&state, &pool.id, body.from_ms, body.to_ms).await? {
            return replay_run(run);
        }
        return Err(ControlError::Internal(anyhow::anyhow!(
            "settle run insert failed"
        )));
    }

    // (3) The books + (4) the position caches run as one phase: any
    // failure tombstones the run ('failed') and fails loudly — the books
    // are append-only, so a partial batch is named by the run row for
    // recovery (documented residual in AUDIT.md A-049).
    let ledger = SqlLedger::new(state.db.clone());
    if let Err(err) = books_and_positions(
        &state,
        &ledger,
        &run_id,
        &positions,
        &attribution_by_depositor,
        &split,
        &pool.id,
    )
    .await
    {
        tombstone_run(&state, &run_id).await;
        return Err(ControlError::Internal(err));
    }

    // Finalize the run: only now does the window replay on retry.
    let finalize = state
        .db
        .sql("UPDATE settle_runs SET status = 'ok' WHERE id = ?");
    if let Err(err) = sqlx::query(sqlx::AssertSqlSafe(finalize.as_str()))
        .bind(&run_id)
        .execute(state.db.pool())
        .await
    {
        tombstone_run(&state, &run_id).await;
        return Err(ControlError::Internal(err.into()));
    }

    tributary_control::audit::record(
        &state.db,
        tributary_control::audit::AuditEntry {
            org_id: Some(&pool.org_id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "depositor.settle",
            target_kind: tributary_control::audit::TargetKind::Pool,
            target_id: &pool.id,
            before: None,
            after: Some(serde_json::json!({
                "run_id": run_id,
                "from_ms": body.from_ms,
                "to_ms": body.to_ms,
                "fee_bps": body.fee_bps,
                "distributable_micros": response.distributable_micros,
                "fee_micros": response.fee_micros,
                "posted": true,
            })),
            ip: None,
        },
    )
    .await;

    Ok(Json(response))
}

/// Replays or refuses a found settle run (A-048): 'ok' runs answer with
/// their stored response — byte-identical to the first answer — anything
/// else is an in-progress/failed window and refuses with 409.
fn replay_run(run: SettleRun) -> ApiResult<Json<SettleResponse>> {
    if run.status == "ok" {
        let response: SettleResponse = serde_json::from_str(&run.response_json)
            .map_err(|e| ControlError::Internal(e.into()))?;
        Ok(Json(response))
    } else {
        Err(ControlError::Conflict(
            "a settle of this exact window is in progress or failed; retry an adjacent \
             window instead (attribution is delta-based — outstanding burns still settle)"
                .into(),
        ))
    }
}

/// The books phase + the position-cache phase of one settle run (A-049,
/// director ruling 2026-11-19: settle posts automatically; positions are
/// true caches — books lead).
///
/// Postings, in the ruled order, every reference naming the run id (one
/// run = one batch of postings): per depositor `capacity.consume`
/// converts the attributed burn from principal claim into the settlement
/// pot (claims.contributed → revenue.usage; the founder-approved M9-D
/// template — root pools have no renter prepayment to fund the pot
/// through prepaid.consume); `settle.fee` takes the operator cut
/// (→ equity.operator, D39); `settle.distribute` moves each payout to
/// `payable.depositor`. Zero-amount rows (a split's floor arithmetic can
/// produce them) do not post — templates are positive-only.
///
/// Then the caches: `earned += payout` and the attributed slice is
/// marked settled via a compare-and-set on `settled_burn_micros`, so a
/// concurrent overlapping settle loses LOUDLY here instead of
/// double-crediting anything.
async fn books_and_positions(
    state: &ControlState,
    ledger: &SqlLedger,
    run_id: &str,
    positions: &[tributary_ledger::settle::BucketPosition],
    attribution_by_depositor: &HashMap<String, i64>,
    split: &tributary_ledger::settle::SettlementSplit,
    pool_id: &str,
) -> Result<(), anyhow::Error> {
    // Accounts open idempotently (open_account is ON CONFLICT DO NOTHING).
    for code in [AccountCode::RevenueUsage, AccountCode::EquityOperator] {
        ledger
            .open_account(code, OwnerKind::Operator, PLATFORM_OWNER_ID)
            .await?;
    }
    for depositor in attribution_by_depositor.keys() {
        ledger
            .open_account(
                AccountCode::PayableDepositor,
                OwnerKind::Depositor,
                depositor,
            )
            .await?;
    }
    let reference = format!("m9.settle.{run_id}");
    for (depositor, amount) in attribution_by_depositor {
        ledger
            .post(
                PostingRequest::new(TemplateCode::CapacityConsume, MicroUsd::new(*amount))
                    .depositor(depositor)
                    .reference(&reference),
            )
            .await?;
    }
    if split.fee.micros() > 0 {
        ledger
            .post(PostingRequest::new(TemplateCode::SettleFee, split.fee).reference(&reference))
            .await?;
    }
    for payout in &split.payouts {
        if payout.amount.micros() <= 0 {
            continue; // zero-payout floor rows exist; postings are positive-only
        }
        ledger
            .post(
                PostingRequest::new(TemplateCode::SettleDistribute, payout.amount)
                    .depositor(&payout.depositor_id)
                    .reference(&reference),
            )
            .await?;
    }

    // The position caches. CAS on settled_burn: the WHERE pins the read
    // basis, the += marks exactly this run's attributed slice.
    let payout_map: HashMap<(String, String, String), i64> = split
        .payouts
        .iter()
        .map(|p| {
            (
                (
                    p.depositor_id.clone(),
                    p.bucket.provider.clone(),
                    p.bucket.model_family.clone(),
                ),
                p.amount.micros(),
            )
        })
        .collect();
    for p in positions {
        let delta = (p.burned_micros.micros() - p.settled_burn_micros).max(0);
        if delta == 0 {
            continue;
        }
        let earned_add = payout_map
            .get(&(
                p.depositor_id.clone(),
                p.bucket.provider.clone(),
                p.bucket.model_family.clone(),
            ))
            .copied()
            .unwrap_or(0);
        let update = state.db.sql(
            "UPDATE depositor_bucket_positions \
             SET earned_micros = earned_micros + ?, \
                 settled_burn_micros = settled_burn_micros + ?, updated_at = ? \
             WHERE pool_id = ? AND depositor_id = ? AND provider = ? AND model_family = ? \
               AND settled_burn_micros = ?",
        );
        let result = sqlx::query(sqlx::AssertSqlSafe(update.as_str()))
            .bind(earned_add)
            .bind(delta)
            .bind(now_ms())
            .bind(pool_id)
            .bind(&p.depositor_id)
            .bind(&p.bucket.provider)
            .bind(&p.bucket.model_family)
            .bind(p.settled_burn_micros)
            .execute(state.db.pool())
            .await?;
        if result.rows_affected() == 0 {
            return Err(anyhow::anyhow!(
                "settle lost the attribution race on position {}/{} of {} — \
                 a concurrent settle touched it; nothing was double-credited",
                p.bucket.provider,
                p.bucket.model_family,
                p.depositor_id
            ));
        }
    }
    Ok(())
}

/// Tombstones a failed run ('failed') — best-effort by design: the error
/// path must never mask its own failure, and the run row + audit trail
/// (the loud 500 the caller returns) name the window to reconcile.
async fn tombstone_run(state: &ControlState, run_id: &str) {
    let sql = state
        .db
        .sql("UPDATE settle_runs SET status = 'failed' WHERE id = ?");
    if let Err(err) = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(run_id)
        .execute(state.db.pool())
        .await
    {
        tracing::error!("settle run {run_id}: tombstone write failed: {err:#}");
    }
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
