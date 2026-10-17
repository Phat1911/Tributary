//! Member self-service (M3, SPEC §7): a member manages their OWN pool keys
//! and reads their OWN usage — and nothing else. Every handler here is
//! scoped by `AuthedUser.user_id` at the QUERY level (`WHERE user_id = me`),
//! and ownership is re-checked on mutations, so one member can never see,
//! let alone touch, another member's keys or metering rows. The isolation
//! tests in `tests/self_service.rs` prove both directions.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::http::StatusCode;

use crate::ControlState;
use crate::audit::{self, AuditEntry, TargetKind};
use crate::auth::{AuthedUser, client_ip};
use crate::db;
use crate::error::{ApiResult, ControlError};
use crate::usage::{PagingQuery, UsageListResponse, UsageSummary, UsageSummaryQuery};

/// GET /api/v1/me/keys — the caller's own keys across pools (masked).
pub async fn my_keys(
    State(state): State<ControlState>,
    user: AuthedUser,
) -> ApiResult<Json<Vec<db::OwnPoolKey>>> {
    let keys = db::pool_keys_by_user(&state.db, &user.user_id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(keys))
}

/// DELETE /api/v1/me/keys/{id} — revoke one of the caller's OWN keys.
///
/// A key owned by anyone else (or nonexistent) is a uniform `404` — the
/// response must not reveal that the key exists at all. Self-revocation is
/// the safety valve for a member who suspects their key leaked.
pub async fn revoke_my_key(
    State(state): State<ControlState>,
    user: AuthedUser,
    headers: HeaderMap,
    Path(key_id): Path<String>,
) -> ApiResult<StatusCode> {
    let (key, owner) = db::pool_key_with_owner(&state.db, &key_id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    if owner.as_deref() != Some(user.user_id.as_str()) {
        return Err(ControlError::NotFound);
    }
    db::revoke_pool_key(&state.db, &key.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: user.org_id.as_deref(),
            actor_id: &user.user_id,
            actor_email: &user.email,
            action: "pool_key.self_revoked",
            target_kind: TargetKind::PoolKey,
            target_id: &key.id,
            // Masked display prefix only — the sanitization invariant.
            before: Some(serde_json::json!({ "status": key.status, "key_prefix": key.key_prefix })),
            after: Some(serde_json::json!({ "status": "revoked", "key_prefix": key.key_prefix })),
            ip: Some(&ip),
        },
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/me/usage — the caller's own metered requests, newest first.
pub async fn my_usage(
    State(state): State<ControlState>,
    user: AuthedUser,
    Query(paging): Query<PagingQuery>,
) -> ApiResult<Json<UsageListResponse>> {
    let filter = db::UsageFilter {
        pool_id: None,
        user_id: Some(&user.user_id),
    };
    // Same sane bounds as the admin list (50 default, ≤200 per page).
    let limit = paging.limit.unwrap_or(50).clamp(1, 200);
    let offset = paging.offset.unwrap_or(0).max(0);
    crate::usage::list(&state, &filter, limit, offset).await
}

/// GET /api/v1/me/usage/summary — daily totals for the caller's own usage.
pub async fn my_usage_summary(
    State(state): State<ControlState>,
    user: AuthedUser,
    Query(query): Query<UsageSummaryQuery>,
) -> ApiResult<Json<UsageSummary>> {
    let filter = db::UsageFilter {
        pool_id: None,
        user_id: Some(&user.user_id),
    };
    crate::usage::summarize(&state, &filter, query.days).await
}
