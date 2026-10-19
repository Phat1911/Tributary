//! `GET /v1/models` — the authed key's pool allowlist (SPEC §7: "filtered
//! by pool allowlist"). Unrestricted pools fall back to config defaults.

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

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
    let models: Vec<String> = if pool.allowlist.is_empty() {
        state.config.default_models.clone()
    } else {
        pool.allowlist.clone()
    };
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
