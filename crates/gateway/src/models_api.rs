//! `GET /v1/models` — static list from config (pool allowlists arrive M2).

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::keys::AuthedKey;
use crate::state::AppState;

pub async fn list_models(State(state): State<AppState>, _auth: AuthedKey) -> Json<Value> {
    let data: Vec<Value> = state
        .config
        .models
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
    Json(json!({ "object": "list", "data": data }))
}
