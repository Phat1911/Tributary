//! Pool-scoped routing: request → pool → provider key selection → decrypt.
//!
//! SPEC §5 lifecycle steps 2–5 live here: resolve the key's pool, enforce
//! the pool's house rules (status + model allowlist), pick a provider key
//! (M2: first-available among active, non-invalid keys — health/weight-aware
//! selection arrives M4+), and decrypt it per-use under `Zeroizing`.

use sqlx::Row;
use tributary_common::db::Db;
use tributary_control::db::ProviderKeyRow;
use zeroize::Zeroizing;

use crate::error::ApiError;

/// Everything the chat path needs to know about a key's pool.
#[derive(Debug, Clone)]
pub struct PoolInfo {
    pub status: String,
    /// Parsed `model_allowlist`; empty = unrestricted.
    pub allowlist: Vec<String>,
}

/// Loads a pool's status + model allowlist.
pub async fn pool_info(db: &Db, pool_id: &str) -> sqlx::Result<Option<PoolInfo>> {
    let sql = db.sql("SELECT status, model_allowlist FROM pools WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_id)
        .fetch_optional(db.pool())
        .await?
        .map(|row| {
            let raw_allowlist: String = row.get("model_allowlist");
            PoolInfo {
                status: row.get("status"),
                allowlist: serde_json::from_str(raw_allowlist.as_str()).unwrap_or_default(),
            }
        });
    Ok(row)
}

/// Selects the pool's next provider key: first active row whose health is
/// not "invalid" (M2 LIGHT health; weighted/health-aware selection is M4+).
pub async fn select_provider_key(db: &Db, pool_id: &str) -> sqlx::Result<Option<ProviderKeyRow>> {
    let sql = db.sql(
        "SELECT id, pool_id, provider, base_url, ciphertext, dek_wrapped, key_prefix, \
          health, last_validated_at, status, created_at \
         FROM provider_keys \
         WHERE pool_id = ? AND status = 'active' AND health != 'invalid' \
         ORDER BY created_at, id \
         LIMIT 1",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_id)
        .fetch_optional(db.pool())
        .await?
        .map(|row| ProviderKeyRow {
            id: row.get("id"),
            pool_id: row.get("pool_id"),
            provider: row.get("provider"),
            base_url: row.get("base_url"),
            ciphertext: row.get("ciphertext"),
            dek_wrapped: row.get("dek_wrapped"),
            key_prefix: row.get("key_prefix"),
            health: row.get("health"),
            last_validated_at: row.get("last_validated_at"),
            status: row.get("status"),
            created_at: row.get("created_at"),
        });
    Ok(row)
}

/// Decrypts the selected provider key (per-use; never cached — AGENTS
/// secrets convention). The plaintext leaves only inside `Zeroizing`.
pub fn decrypt_provider_key(
    master: &tributary_crypto::MasterKey,
    row: &ProviderKeyRow,
) -> Result<Zeroizing<String>, ApiError> {
    tributary_control::api::decrypt_provider_key(master, row).map_err(|e| {
        ApiError::Internal(anyhow::anyhow!(
            "provider key {prefix} failed to decrypt: {e:#}",
            prefix = row.key_prefix
        ))
    })
}
