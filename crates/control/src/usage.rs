//! Usage endpoints (M3, SPEC §7 `/api/v1/usage`): per-pool (optionally
//! per-member) metering for admins, plus the shared list/summary builders
//! the member self-service endpoints reuse. All money stays integer
//! micro-USD (`cost_micros`) end to end — no floats, ever.

use axum::Json;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde::Serialize;

use crate::ControlState;
use crate::auth::AuthedAdmin;
use crate::db::{self, UsageFilter};
use crate::error::{ApiResult, ControlError};

/// GET /api/v1/usage?pool_id=…&user_id=…&limit=&offset= — pool_id is
/// REQUIRED for the admin list: the pool is the org-scope anchor (its org
/// decides visibility); user_id optionally narrows to one member in it.
#[derive(Debug, Deserialize)] // serde: query string → this struct
pub struct UsageQuery {
    pub pool_id: String,
    pub user_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Paging params shared by admin + self-service usage lists.
#[derive(Debug, Deserialize)]
pub struct PagingQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// GET /api/v1/usage/summary?pool_id=…&user_id=…&days=…
#[derive(Debug, Deserialize)]
pub struct UsageSummaryQuery {
    /// Omit for an org-wide (or instance-wide, host) summary.
    pub pool_id: Option<String>,
    pub user_id: Option<String>,
    /// Window length in days (default 14, capped at 90 — a viewer, not a
    /// data-export tool).
    pub days: Option<i64>,
}

/// The usage list envelope.
#[derive(Debug, Serialize)] // serde: this struct → JSON
pub struct UsageListResponse {
    pub events: Vec<db::UsageEventPublic>,
}

/// One point of the sparkline + the window totals.
#[derive(Debug, Serialize)]
pub struct UsageSummary {
    pub days: Vec<db::DailyUsage>,
    pub totals: UsageTotals,
}

#[derive(Debug, Serialize, Default)]
pub struct UsageTotals {
    pub requests: i64,
    pub tokens: i64,
}

/// Clamps paging params to sane bounds (portable LIMIT/OFFSET ≥ 0).
fn clamp_page(limit: Option<i64>, offset: Option<i64>) -> (i64, i64) {
    (
        limit.unwrap_or(50).clamp(1, 200),
        offset.unwrap_or(0).max(0),
    )
}

/// Clamps the summary window (default 2 weeks, ≤ 90 days).
fn clamp_days(days: Option<i64>) -> i64 {
    days.unwrap_or(14).clamp(1, 90)
}

/// Loads + scopes one pool for an admin (the scope anchor for usage reads).
async fn scoped_pool(
    state: &ControlState,
    admin: &AuthedAdmin,
    pool_id: &str,
) -> ApiResult<db::PoolRow> {
    let pool = db::pool_by_id(&state.db, pool_id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    admin.ensure_org_scope(&pool.org_id)?;
    Ok(pool)
}

/// GET /api/v1/usage — usage events for one pool, optionally one member.
pub async fn list_for_pool(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Query(query): Query<UsageQuery>,
) -> ApiResult<Json<UsageListResponse>> {
    let pool = scoped_pool(&state, &admin, &query.pool_id).await?;
    let filter = UsageFilter {
        pool_id: Some(&pool.id),
        user_id: query.user_id.as_deref(),
    };
    let (limit, offset) = clamp_page(query.limit, query.offset);
    list(&state, &filter, limit, offset).await
}

/// Shared list builder (admin pool view + member self-service).
pub async fn list(
    state: &ControlState,
    filter: &UsageFilter<'_>,
    limit: i64,
    offset: i64,
) -> ApiResult<Json<UsageListResponse>> {
    let events = db::usage_events(&state.db, filter, limit, offset)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(UsageListResponse { events }))
}

/// GET /api/v1/usage/summary — daily aggregates + window totals. Without a
/// pool_id an org_admin gets their whole org (merged pool-by-pool so
/// cross-org rows can never join the series); host gets the instance.
pub async fn summary(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Query(query): Query<UsageSummaryQuery>,
) -> ApiResult<Json<UsageSummary>> {
    if let Some(pool_id) = &query.pool_id {
        // One named pool: scope-check it, then aggregate exactly it.
        let pool = scoped_pool(&state, &admin, pool_id).await?;
        let filter = UsageFilter {
            pool_id: Some(&pool.id),
            user_id: query.user_id.as_deref(),
        };
        return summarize(&state, &filter, query.days).await;
    }
    if admin.role != "host_admin" {
        // Org-wide: aggregate each pool the admin CAN see, then merge —
        // scope is enforced by construction, not by filtering after.
        let pools = db::list_pools(&state.db, admin.org_id.as_deref())
            .await
            .map_err(|e| ControlError::Internal(e.into()))?;
        let mut merged: Vec<db::DailyUsage> = Vec::new();
        let since = since_ms(query.days);
        for pool in pools {
            let filter = UsageFilter {
                pool_id: Some(&pool.id),
                user_id: query.user_id.as_deref(),
            };
            let part = db::usage_daily(&state.db, &filter, since)
                .await
                .map_err(|e| ControlError::Internal(e.into()))?;
            merge_days(&mut merged, part);
        }
        merged.sort_by_key(|d| d.day_start_ms);
        return Ok(Json(totals_of(merged)));
    }
    // Host, no pool: the whole instance in one aggregate.
    let filter = UsageFilter {
        pool_id: None,
        user_id: query.user_id.as_deref(),
    };
    summarize(&state, &filter, query.days).await
}

/// Shared summary builder (admin + member self-service).
pub async fn summarize(
    state: &ControlState,
    filter: &UsageFilter<'_>,
    days: Option<i64>,
) -> ApiResult<Json<UsageSummary>> {
    let series = db::usage_daily(&state.db, filter, since_ms(days))
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(totals_of(series)))
}

/// Window start for a `days`-long summary ending now.
fn since_ms(days: Option<i64>) -> i64 {
    tributary_common::db::now_ms() - clamp_days(days) * 86_400_000
}

/// Folds one pool's daily rows into the merged org-wide series.
fn merge_days(merged: &mut Vec<db::DailyUsage>, part: Vec<db::DailyUsage>) {
    for day in part {
        match merged
            .iter_mut()
            .find(|m| m.day_start_ms == day.day_start_ms)
        {
            Some(slot) => {
                slot.requests += day.requests;
                slot.tokens += day.tokens;
            }
            None => merged.push(day),
        }
    }
}

/// Wraps a daily series with its window totals.
fn totals_of(days: Vec<db::DailyUsage>) -> UsageSummary {
    let totals = days.iter().fold(UsageTotals::default(), |mut acc, d| {
        acc.requests += d.requests;
        acc.tokens += d.tokens;
        acc
    });
    UsageSummary { days, totals }
}
