//! `GET /v1/models` — the authed key's pool allowlist (SPEC §7: "filtered
//! by pool allowlist"). Unrestricted pools fall back to config defaults.
//!
//! M4: canary model aliases are merged into the listing exactly like real
//! models (same object shape, same `owned_by`) — the bait has to swim with
//! the school. A member sees pool-wide aliases plus their own scoped ones.

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::db;
use crate::error::ApiError;
use crate::keys::AuthedKey;
use crate::routing;
use crate::state::AppState;

pub async fn list_models(
    State(state): State<AppState>,
    auth: AuthedKey,
) -> Result<Json<Value>, ApiError> {
    let pool = routing::pool_info(&state.db, &auth.pool_id)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("pool lookup failed: {e}")))?
        .ok_or(ApiError::Unauthorized)?;
    let mut models: Vec<String> = if pool.allowlist.is_empty() {
        state.config.default_models.clone()
    } else {
        pool.allowlist.clone()
    };
    // Canary aliases: pool-wide rows plus this member's scoped rows.
    // Deduped so an alias that collides with a real allowlist entry does
    // not double-list (the canary check still wins at request time).
    let canaries = db::canary_models_for_member(&state.db, &auth.pool_id, auth.user_id.as_deref())
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("canary model lookup failed: {e}")))?;
    for alias in canaries {
        if !models.contains(&alias) {
            models.push(alias);
        }
    }
    let data: Vec<Value> = models
        .iter()
        .map(|id| {
            json!({
                "id": id,
                "object": "model",
                "created": 0,
                "owned_by": "tributary",
            })
        })
        .collect();
    Ok(Json(json!({ "object": "list", "data": data })))
}
