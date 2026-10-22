//! Control-plane queries (portable `?`-SQL via `tributary_common::db::Db`).
//!
//! Row types that cross into responses are split: full rows (for internal
//! use, e.g. decrypting a provider key) vs. the API's masked views — the
//! handlers never serialize `password_hash` or ciphertext columns.

use serde::Serialize;
use serde_json::Value;
use sqlx::Row;
use tributary_common::db::Db;

// ---------------------------------------------------------------------------
// Organizations
// ---------------------------------------------------------------------------

/// A full `organizations` row.
#[derive(Debug, Clone)]
pub struct OrgRow {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub settings: String,
    pub created_at: i64,
}

/// Inserts one organization row.
pub async fn insert_org(db: &Db, org: &OrgRow) -> sqlx::Result<()> {
    let sql = db.sql(
        "INSERT INTO organizations (id, name, slug, settings, created_at) \
         VALUES (?, ?, ?, ?, ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(&org.id)
        .bind(&org.name)
        .bind(&org.slug)
        .bind(&org.settings)
        .bind(org.created_at)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// Lists organizations visible to `(role, org_id)`: all for host_admin,
/// only their own for org_admin.
pub async fn list_orgs(db: &Db, role: &str, org_id: Option<&str>) -> sqlx::Result<Vec<OrgRow>> {
    let (sql, filter) = if role == "host_admin" {
        (
            "SELECT id, name, slug, settings, created_at FROM organizations ORDER BY created_at",
            None,
        )
    } else {
        (
            "SELECT id, name, slug, settings, created_at FROM organizations \
             WHERE id = ? ORDER BY created_at",
            org_id,
        )
    };
    let sql = db.sql(sql);
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()));
    if let Some(id) = filter {
        q = q.bind(id);
    }
    let rows = q
        .fetch_all(db.pool())
        .await?
        .into_iter()
        .map(org_from_row)
        .collect();
    Ok(rows)
}

/// Fetches one organization by id.
pub async fn org_by_id(db: &Db, id: &str) -> sqlx::Result<Option<OrgRow>> {
    let sql = db.sql("SELECT id, name, slug, settings, created_at FROM organizations WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .fetch_optional(db.pool())
        .await?
        .map(org_from_row);
    Ok(row)
}

/// Maps a SELECT row into an [`OrgRow`] (shared by list/get).
fn org_from_row(row: sqlx::any::AnyRow) -> OrgRow {
    OrgRow {
        id: row.get("id"),
        name: row.get("name"),
        slug: row.get("slug"),
        settings: row.get("settings"),
        created_at: row.get("created_at"),
    }
}

/// Counts an org's pools (delete guard).
pub async fn count_pools_for_org(db: &Db, org_id: &str) -> sqlx::Result<i64> {
    let sql = db.sql("SELECT COUNT(*) AS n FROM pools WHERE org_id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(org_id)
        .fetch_one(db.pool())
        .await?;
    {
        let n: i64 = row.get("n");
        Ok(n)
    }
}

/// Hard-deletes an org row (only called after the pool guard passes).
pub async fn delete_org(db: &Db, id: &str) -> sqlx::Result<()> {
    let sql = db.sql("DELETE FROM organizations WHERE id = ?");
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Users (admins + members) and sessions
// ---------------------------------------------------------------------------

/// A full `users` row — internal only (handlers never serialize the hash).
#[derive(Debug, Clone)]
pub struct UserRow {
    pub id: String,
    pub email: String,
    pub password_hash: String,
    pub role: String,
    pub org_id: Option<String>,
    pub created_at: i64,
}

/// A `users` row without the password hash — safe for API responses.
#[derive(Debug, Clone, Serialize)] // serde: this struct → JSON — PublicUser carries no hash
pub struct PublicUser {
    pub id: String,
    pub email: String,
    pub role: String,
    pub org_id: Option<String>,
    pub created_at: i64,
}

impl From<&UserRow> for PublicUser {
    /// Strips the PHC hash when crossing into response territory.
    fn from(u: &UserRow) -> Self {
        Self {
            id: u.id.clone(),
            email: u.email.clone(),
            role: u.role.clone(),
            org_id: u.org_id.clone(),
            created_at: u.created_at,
        }
    }
}

/// Inserts one user row.
pub async fn insert_user(db: &Db, user: &UserRow) -> sqlx::Result<()> {
    let sql = db.sql(
        "INSERT INTO users (id, email, password_hash, role, org_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(&user.id)
        .bind(&user.email)
        .bind(&user.password_hash)
        .bind(&user.role)
        .bind(user.org_id.as_deref())
        .bind(user.created_at)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// Looks a user up by (already lowercased) email — the login path.
pub async fn user_by_email(db: &Db, email: &str) -> sqlx::Result<Option<UserRow>> {
    let sql = db.sql(
        "SELECT id, email, password_hash, role, org_id, created_at FROM users WHERE email = ?",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(email)
        .fetch_optional(db.pool())
        .await?
        .map(user_from_row);
    Ok(row)
}

/// Fetches one user by id.
pub async fn user_by_id(db: &Db, id: &str) -> sqlx::Result<Option<UserRow>> {
    let sql =
        db.sql("SELECT id, email, password_hash, role, org_id, created_at FROM users WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .fetch_optional(db.pool())
        .await?
        .map(user_from_row);
    Ok(row)
}

/// Lists an org's users (members view for org admins).
pub async fn users_by_org(db: &Db, org_id: &str) -> sqlx::Result<Vec<UserRow>> {
    let sql = db.sql(
        "SELECT id, email, password_hash, role, org_id, created_at FROM users \
         WHERE org_id = ? ORDER BY created_at",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(org_id)
        .fetch_all(db.pool())
        .await?
        .into_iter()
        .map(user_from_row)
        .collect();
    Ok(rows)
}

/// Counts all users — the host-admin bootstrap guard.
pub async fn count_users(db: &Db) -> sqlx::Result<i64> {
    let sql = db.sql("SELECT COUNT(*) AS n FROM users");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .fetch_one(db.pool())
        .await?;
    {
        let n: i64 = row.get("n");
        Ok(n)
    }
}

/// Revokes every pool key a member holds, then deletes the user.
///
/// Two steps so a deleted member cannot keep riding an issued key (there are
/// no FKs in the schema — portability choice — so ordering is on us).
pub async fn delete_member(db: &Db, user_id: &str) -> sqlx::Result<()> {
    let mut tx = db.pool().begin().await?;
    let sql = db.sql("UPDATE pool_keys SET status = 'revoked' WHERE user_id = ?");
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let sql = db.sql("DELETE FROM users WHERE id = ?");
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Maps a SELECT row into a [`UserRow`].
fn user_from_row(row: sqlx::any::AnyRow) -> UserRow {
    UserRow {
        id: row.get("id"),
        email: row.get("email"),
        password_hash: row.get("password_hash"),
        role: row.get("role"),
        org_id: row.get("org_id"),
        created_at: row.get("created_at"),
    }
}

/// A live session joined with its user (the extractor's lookup result).
#[derive(Debug, Clone)]
pub struct SessionUserRow {
    pub session_id: String,
    pub user_id: String,
    /// Snapshot for audit rows / `GET /auth/me`.
    pub email: String,
    pub role: String,
    pub org_id: Option<String>,
    pub expires_at: i64,
}

/// Resolves a session token hash to its (session, user) pair.
pub async fn session_user_by_token_hash(
    db: &Db,
    token_hash: &str,
) -> sqlx::Result<Option<SessionUserRow>> {
    let sql = db.sql(
        "SELECT s.id AS session_id, s.expires_at, u.id AS user_id, u.email, u.role, u.org_id \
         FROM sessions s JOIN users u ON s.user_id = u.id \
         WHERE s.token_hash = ?",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(token_hash)
        .fetch_optional(db.pool())
        .await?
        .map(|row| SessionUserRow {
            session_id: row.get("session_id"),
            user_id: row.get("user_id"),
            email: row.get("email"),
            role: row.get("role"),
            org_id: row.get("org_id"),
            expires_at: row.get("expires_at"),
        });
    Ok(row)
}

/// Inserts a new session row (token hash + expiry).
pub async fn insert_session(
    db: &Db,
    id: &str,
    user_id: &str,
    token_hash: &str,
    created_at: i64,
    expires_at: i64,
) -> sqlx::Result<()> {
    let sql = db.sql(
        "INSERT INTO sessions (id, user_id, token_hash, created_at, expires_at) \
         VALUES (?, ?, ?, ?, ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .bind(user_id)
        .bind(token_hash)
        .bind(created_at)
        .bind(expires_at)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// Deletes one session (logout / lazy expiry sweep).
pub async fn delete_session(db: &Db, id: &str) -> sqlx::Result<()> {
    let sql = db.sql("DELETE FROM sessions WHERE id = ?");
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Pools
// ---------------------------------------------------------------------------

/// A `pools` row.
#[derive(Debug, Clone)]
pub struct PoolRow {
    pub id: String,
    pub org_id: String,
    pub name: String,
    pub kind: String,
    /// JSON-array TEXT column — parse with serde_json at the boundary.
    pub model_allowlist: String,
    pub status: String,
    pub currency: String,
    pub created_at: i64,
}

/// Inserts one pool row.
pub async fn insert_pool(db: &Db, pool: &PoolRow) -> sqlx::Result<()> {
    let sql = db.sql(
        "INSERT INTO pools (id, org_id, name, kind, model_allowlist, status, currency, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(&pool.id)
        .bind(&pool.org_id)
        .bind(&pool.name)
        .bind(&pool.kind)
        .bind(&pool.model_allowlist)
        .bind(&pool.status)
        .bind(&pool.currency)
        .bind(pool.created_at)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// Lists pools, optionally filtered to one org (org_admin scope).
pub async fn list_pools(db: &Db, org_id: Option<&str>) -> sqlx::Result<Vec<PoolRow>> {
    let (sql, bind_org) = if org_id.is_some() {
        (
            "SELECT id, org_id, name, kind, model_allowlist, status, currency, created_at \
             FROM pools WHERE org_id = ? ORDER BY created_at",
            true,
        )
    } else {
        (
            "SELECT id, org_id, name, kind, model_allowlist, status, currency, created_at \
             FROM pools ORDER BY created_at",
            false,
        )
    };
    let sql = db.sql(sql);
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()));
    if bind_org {
        q = q.bind(org_id.unwrap());
    }
    let rows = q
        .fetch_all(db.pool())
        .await?
        .into_iter()
        .map(pool_from_row)
        .collect();
    Ok(rows)
}

/// Fetches one pool by id.
pub async fn pool_by_id(db: &Db, id: &str) -> sqlx::Result<Option<PoolRow>> {
    let sql = db.sql(
        "SELECT id, org_id, name, kind, model_allowlist, status, currency, created_at \
         FROM pools WHERE id = ?",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .fetch_optional(db.pool())
        .await?
        .map(pool_from_row);
    Ok(row)
}

/// Soft-disables a pool (history is never hard-deleted — usage rows persist).
pub async fn disable_pool(db: &Db, id: &str) -> sqlx::Result<u64> {
    let sql = db.sql("UPDATE pools SET status = 'disabled' WHERE id = ? AND status = 'active'");
    let res = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(res.rows_affected())
}

/// Maps a SELECT row into a [`PoolRow`].
fn pool_from_row(row: sqlx::any::AnyRow) -> PoolRow {
    PoolRow {
        id: row.get("id"),
        org_id: row.get("org_id"),
        name: row.get("name"),
        kind: row.get("kind"),
        model_allowlist: row.get("model_allowlist"),
        status: row.get("status"),
        currency: row.get("currency"),
        created_at: row.get("created_at"),
    }
}

// ---------------------------------------------------------------------------
// Provider keys
// ---------------------------------------------------------------------------

/// A full `provider_keys` row, including encrypted blobs — internal use only.
#[derive(Debug, Clone)]
pub struct ProviderKeyRow {
    pub id: String,
    pub pool_id: String,
    pub provider: String,
    pub base_url: String,
    pub ciphertext: String,
    pub dek_wrapped: String,
    pub key_prefix: String,
    pub health: String,
    pub last_validated_at: Option<i64>,
    pub status: String,
    pub created_at: i64,
}

/// The masked view served by the API — no ciphertext ever leaves the crate.
#[derive(Debug, Clone, Serialize)] // serde: this struct → JSON — masked, no ciphertext
pub struct PublicProviderKey {
    pub id: String,
    pub pool_id: String,
    pub provider: String,
    pub base_url: String,
    pub key_prefix: String,
    pub health: String,
    pub last_validated_at: Option<i64>,
    pub status: String,
    pub created_at: i64,
}

impl From<&ProviderKeyRow> for PublicProviderKey {
    /// Strips both encrypted blobs for response serialization.
    fn from(k: &ProviderKeyRow) -> Self {
        Self {
            id: k.id.clone(),
            pool_id: k.pool_id.clone(),
            provider: k.provider.clone(),
            base_url: k.base_url.clone(),
            key_prefix: k.key_prefix.clone(),
            health: k.health.clone(),
            last_validated_at: k.last_validated_at,
            status: k.status.clone(),
            created_at: k.created_at,
        }
    }
}

/// Inserts a provider-key row with its sealed envelope.
pub async fn insert_provider_key(db: &Db, key: &ProviderKeyRow) -> sqlx::Result<()> {
    let sql = db.sql(
        "INSERT INTO provider_keys \
         (id, pool_id, provider, base_url, ciphertext, dek_wrapped, key_prefix, \
          health, last_validated_at, status, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(&key.id)
        .bind(&key.pool_id)
        .bind(&key.provider)
        .bind(&key.base_url)
        .bind(&key.ciphertext)
        .bind(&key.dek_wrapped)
        .bind(&key.key_prefix)
        .bind(&key.health)
        .bind(key.last_validated_at)
        .bind(&key.status)
        .bind(key.created_at)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// Lists a pool's provider keys (full rows; mask before serializing).
pub async fn provider_keys_by_pool(db: &Db, pool_id: &str) -> sqlx::Result<Vec<ProviderKeyRow>> {
    let sql = db.sql(
        "SELECT id, pool_id, provider, base_url, ciphertext, dek_wrapped, key_prefix, \
          health, last_validated_at, status, created_at \
         FROM provider_keys WHERE pool_id = ? ORDER BY created_at",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_id)
        .fetch_all(db.pool())
        .await?
        .into_iter()
        .map(provider_key_from_row)
        .collect();
    Ok(rows)
}

/// Fetches one provider key (full row — needed for decryption).
pub async fn provider_key_by_id(db: &Db, id: &str) -> sqlx::Result<Option<ProviderKeyRow>> {
    let sql = db.sql(
        "SELECT id, pool_id, provider, base_url, ciphertext, dek_wrapped, key_prefix, \
          health, last_validated_at, status, created_at \
         FROM provider_keys WHERE id = ?",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .fetch_optional(db.pool())
        .await?
        .map(provider_key_from_row);
    Ok(row)
}

/// Updates health after a validation probe.
pub async fn set_provider_key_health(
    db: &Db,
    id: &str,
    health: &str,
    validated_at: i64,
) -> sqlx::Result<()> {
    let sql = db.sql("UPDATE provider_keys SET health = ?, last_validated_at = ? WHERE id = ?");
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(health)
        .bind(validated_at)
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// Revokes a provider key (soft delete — usage rows keep referencing it).
pub async fn revoke_provider_key(db: &Db, id: &str) -> sqlx::Result<u64> {
    let sql =
        db.sql("UPDATE provider_keys SET status = 'revoked' WHERE id = ? AND status = 'active'");
    let res = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(res.rows_affected())
}

/// Maps a SELECT row into a [`ProviderKeyRow`].
fn provider_key_from_row(row: sqlx::any::AnyRow) -> ProviderKeyRow {
    ProviderKeyRow {
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
    }
}

// ---------------------------------------------------------------------------
// Pool keys (issuance from the control plane)
// ---------------------------------------------------------------------------

/// A new pool key binding a member to a pool.
#[derive(Debug, Clone)]
pub struct NewPoolKey {
    pub id: String,
    pub pool_id: String,
    pub key_hash: String,
    pub key_prefix: String,
    pub user_id: String,
    /// Optional expiry (epoch ms; None = never) — M4 key governance.
    pub expires_at: Option<i64>,
    pub created_at: i64,
}

/// Inserts an issued pool key.
pub async fn insert_issued_pool_key(db: &Db, key: &NewPoolKey) -> sqlx::Result<()> {
    let sql = db.sql(
        "INSERT INTO pool_keys \
         (id, pool_id, key_hash, key_prefix, kind, status, user_id, expires_at, created_at) \
         VALUES (?, ?, ?, ?, 'live', 'active', ?, ?, ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(&key.id)
        .bind(&key.pool_id)
        .bind(&key.key_hash)
        .bind(&key.key_prefix)
        .bind(&key.user_id)
        .bind(key.expires_at)
        .bind(key.created_at)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// The masked pool-key view for listings (hash never serialized).
#[derive(Debug, Clone, Serialize)] // serde: this struct → JSON — masked, no key hash
pub struct PublicPoolKey {
    pub id: String,
    pub pool_id: String,
    pub key_prefix: String,
    pub user_id: Option<String>,
    pub status: String,
    pub created_at: i64,
}

/// Lists a pool's keys (masked).
pub async fn pool_keys_by_pool(db: &Db, pool_id: &str) -> sqlx::Result<Vec<PublicPoolKey>> {
    let sql = db.sql(
        "SELECT id, pool_id, key_prefix, user_id, status, created_at \
         FROM pool_keys WHERE pool_id = ? ORDER BY created_at",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_id)
        .fetch_all(db.pool())
        .await?
        .into_iter()
        .map(|row| PublicPoolKey {
            id: row.get("id"),
            pool_id: row.get("pool_id"),
            key_prefix: row.get("key_prefix"),
            user_id: row.get("user_id"),
            status: row.get("status"),
            created_at: row.get("created_at"),
        })
        .collect();
    Ok(rows)
}

/// Revokes a pool key (soft delete — usage rows keep referencing it).
/// M4: also revocable from 'suspended' (the review queue's manual kill
/// switch for an auto-blocked key).
pub async fn revoke_pool_key(db: &Db, id: &str) -> sqlx::Result<u64> {
    let sql = db.sql(
        "UPDATE pool_keys SET status = 'revoked' \
         WHERE id = ? AND status IN ('active', 'suspended')",
    );
    let res = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(res.rows_affected())
}

// ---------------------------------------------------------------------------
// Quota rules
// ---------------------------------------------------------------------------

/// A `quota_rules` row (rule 4 — enforced later at the gateway, stored here).
#[derive(Debug, Clone, Serialize)] // serde: this struct → JSON — rule view for listings
pub struct QuotaRuleRow {
    pub id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub period: String,
    pub limit_tokens: Option<i64>,
    pub limit_requests: Option<i64>,
    pub limit_cost: i64,
    pub created_at: i64,
}

/// Atomically replaces the rule for (subject, period) — delete+insert in one
/// transaction, avoiding any ON CONFLICT dialect differences between
/// SQLite and PostgreSQL.
pub async fn upsert_quota_rule(db: &Db, rule: &QuotaRuleRow) -> sqlx::Result<()> {
    let mut tx = db.pool().begin().await?;
    let del = db.sql(
        "DELETE FROM quota_rules \
         WHERE subject_kind = ? AND subject_id = ? AND period = ?",
    );
    sqlx::query(sqlx::AssertSqlSafe(del.as_str()))
        .bind(&rule.subject_kind)
        .bind(&rule.subject_id)
        .bind(&rule.period)
        .execute(&mut *tx)
        .await?;
    let ins = db.sql(
        "INSERT INTO quota_rules \
         (id, subject_kind, subject_id, period, limit_tokens, limit_requests, limit_cost, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(ins.as_str()))
        .bind(&rule.id)
        .bind(&rule.subject_kind)
        .bind(&rule.subject_id)
        .bind(&rule.period)
        .bind(rule.limit_tokens)
        .bind(rule.limit_requests)
        .bind(rule.limit_cost)
        .bind(rule.created_at)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Lists all quota rules (callers filter by org scope after resolving
/// subject ownership — M2-scale lists are tiny).
pub async fn list_quota_rules(db: &Db) -> sqlx::Result<Vec<QuotaRuleRow>> {
    let sql = db.sql(
        "SELECT id, subject_kind, subject_id, period, limit_tokens, limit_requests, \
          limit_cost, created_at FROM quota_rules ORDER BY created_at",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .fetch_all(db.pool())
        .await?
        .into_iter()
        .map(quota_rule_from_row)
        .collect();
    Ok(rows)
}

/// Fetches one quota rule by id.
pub async fn quota_rule_by_id(db: &Db, id: &str) -> sqlx::Result<Option<QuotaRuleRow>> {
    let sql = db.sql(
        "SELECT id, subject_kind, subject_id, period, limit_tokens, limit_requests, \
          limit_cost, created_at FROM quota_rules WHERE id = ?",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .fetch_optional(db.pool())
        .await?
        .map(quota_rule_from_row);
    Ok(row)
}

/// Deletes one quota rule.
pub async fn delete_quota_rule(db: &Db, id: &str) -> sqlx::Result<u64> {
    let sql = db.sql("DELETE FROM quota_rules WHERE id = ?");
    let res = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(res.rows_affected())
}

/// Maps a SELECT row into a [`QuotaRuleRow`].
fn quota_rule_from_row(row: sqlx::any::AnyRow) -> QuotaRuleRow {
    QuotaRuleRow {
        id: row.get("id"),
        subject_kind: row.get("subject_kind"),
        subject_id: row.get("subject_id"),
        period: row.get("period"),
        limit_tokens: row.get("limit_tokens"),
        limit_requests: row.get("limit_requests"),
        limit_cost: row.get("limit_cost"),
        created_at: row.get("created_at"),
    }
}

/// Resolves which org a quota-rule subject belongs to (for scope checks).
///
/// `None` = not found, or a subject outside any org (then only host_admin
/// may manage its rules).
pub async fn subject_org(db: &Db, kind: &str, id: &str) -> sqlx::Result<Option<String>> {
    let sql = match kind {
        "pool" => db.sql("SELECT org_id FROM pools WHERE id = ?"),
        "user" => db.sql("SELECT org_id AS org_id FROM users WHERE id = ?"),
        "pool_key" => db.sql(
            "SELECT p.org_id AS org_id FROM pool_keys k JOIN pools p ON k.pool_id = p.id \
             WHERE k.id = ?",
        ),
        _ => return Ok(None),
    };
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .fetch_optional(db.pool())
        .await?
        .map(|row| {
            let org: Option<String> = row.get("org_id");
            org
        });
    Ok(row.flatten())
}

// ---------------------------------------------------------------------------
// Usage queries (M3 dashboards)
// ---------------------------------------------------------------------------

/// A `usage_events` row as served to dashboards — metrics + ids only, with
/// the pool key's MASKED display prefix joined in (never the hash).
#[derive(Debug, Clone, Serialize)] // serde: this struct → JSON — usage table row shape
pub struct UsageEventPublic {
    pub id: String,
    pub ts: i64,
    pub pool_id: Option<String>,
    pub pool_name: Option<String>,
    pub pool_key_id: String,
    pub key_prefix: String,
    pub user_id: Option<String>,
    pub model: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cost_micros: i64,
    pub latency_ms: i64,
    pub status: String,
}

/// Which slice of `usage_events` a caller reads: one pool (admin view,
/// optionally one member within it) or one user across pools (self-service).
#[derive(Debug, Clone, Default)]
pub struct UsageFilter<'a> {
    pub pool_id: Option<&'a str>,
    pub user_id: Option<&'a str>,
}

/// Lists usage events newest-first with pagination (LIMIT/OFFSET are the
/// same on both backends; the WHERE list is assembled before the portable
/// `?`-rewrite runs, so bind order still matches placeholder order).
pub async fn usage_events(
    db: &Db,
    filter: &UsageFilter<'_>,
    limit: i64,
    offset: i64,
) -> sqlx::Result<Vec<UsageEventPublic>> {
    let mut sql = String::from(
        "SELECT e.id, e.ts, e.pool_id, p.name AS pool_name, e.pool_key_id, k.key_prefix, \
          e.user_id, e.model, e.prompt_tokens, e.completion_tokens, e.cost_micros, \
          e.latency_ms, e.status \
         FROM usage_events e \
         JOIN pool_keys k ON e.pool_key_id = k.id \
         LEFT JOIN pools p ON e.pool_id = p.id",
    );
    let mut clauses: Vec<&'static str> = Vec::new();
    if filter.pool_id.is_some() {
        clauses.push("e.pool_id = ?");
    }
    if filter.user_id.is_some() {
        clauses.push("e.user_id = ?");
    }
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    sql.push_str(" ORDER BY e.ts DESC, e.id LIMIT ? OFFSET ?");
    let sql = db.sql(&sql);
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()));
    if let Some(pool) = filter.pool_id {
        q = q.bind(pool);
    }
    if let Some(user) = filter.user_id {
        q = q.bind(user);
    }
    let rows = q
        .bind(limit)
        .bind(offset)
        .fetch_all(db.pool())
        .await?
        .into_iter()
        .map(|row| UsageEventPublic {
            id: row.get("id"),
            ts: row.get("ts"),
            pool_id: row.get("pool_id"),
            pool_name: row.get("pool_name"),
            pool_key_id: row.get("pool_key_id"),
            key_prefix: row.get("key_prefix"),
            user_id: row.get("user_id"),
            model: row.get("model"),
            prompt_tokens: row.get("prompt_tokens"),
            completion_tokens: row.get("completion_tokens"),
            cost_micros: row.get("cost_micros"),
            latency_ms: row.get("latency_ms"),
            status: row.get("status"),
        })
        .collect();
    Ok(rows)
}

/// One UTC day of usage totals (the sparkline's data point).
#[derive(Debug, Clone, Serialize)] // serde: this struct → JSON — sparkline point
pub struct DailyUsage {
    /// Unix epoch ms of the UTC day's start (the x-axis key).
    pub day_start_ms: i64,
    pub requests: i64,
    /// prompt + completion tokens.
    pub tokens: i64,
}

/// Aggregates usage per UTC day since `since_ms` for the same filter shape
/// as [`usage_events`]. `ts / 86400000` is integer division on BOTH backends
/// (SQLite int/int, PG bigint/bigint) — the M2 CAST rule still wraps SUM.
pub async fn usage_daily(
    db: &Db,
    filter: &UsageFilter<'_>,
    since_ms: i64,
) -> sqlx::Result<Vec<DailyUsage>> {
    let mut sql = String::from(
        "SELECT CAST(ts / 86400000 AS BIGINT) AS day_idx, COUNT(*) AS requests, \
          CAST(COALESCE(SUM(prompt_tokens + completion_tokens), 0) AS BIGINT) AS tokens \
         FROM usage_events",
    );
    let mut clauses: Vec<&'static str> = Vec::new();
    if filter.pool_id.is_some() {
        clauses.push("pool_id = ?");
    }
    if filter.user_id.is_some() {
        clauses.push("user_id = ?");
    }
    clauses.push("ts >= ?");
    sql.push_str(" WHERE ");
    sql.push_str(&clauses.join(" AND "));
    sql.push_str(" GROUP BY day_idx ORDER BY day_idx");
    let sql = db.sql(&sql);
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()));
    if let Some(pool) = filter.pool_id {
        q = q.bind(pool);
    }
    if let Some(user) = filter.user_id {
        q = q.bind(user);
    }
    let rows = q
        .bind(since_ms)
        .fetch_all(db.pool())
        .await?
        .into_iter()
        .map(|row| {
            let day_idx: i64 = row.get("day_idx");
            DailyUsage {
                day_start_ms: day_idx * 86_400_000,
                requests: row.get("requests"),
                tokens: row.get("tokens"),
            }
        })
        .collect();
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Member self-service (M3): own pool keys, by user
// ---------------------------------------------------------------------------

/// A member's own pool key with its pool name for display.
#[derive(Debug, Clone, Serialize)] // serde: this struct → JSON — self-service key row
pub struct OwnPoolKey {
    pub id: String,
    pub pool_id: String,
    pub pool_name: String,
    pub key_prefix: String,
    pub status: String,
    pub created_at: i64,
}

/// Lists one user's pool keys across pools (masked — hash never selected).
pub async fn pool_keys_by_user(db: &Db, user_id: &str) -> sqlx::Result<Vec<OwnPoolKey>> {
    let sql = db.sql(
        "SELECT k.id, k.pool_id, p.name AS pool_name, k.key_prefix, k.status, k.created_at \
         FROM pool_keys k JOIN pools p ON k.pool_id = p.id \
         WHERE k.user_id = ? ORDER BY k.created_at",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(user_id)
        .fetch_all(db.pool())
        .await?
        .into_iter()
        .map(|row| OwnPoolKey {
            id: row.get("id"),
            pool_id: row.get("pool_id"),
            pool_name: row.get("pool_name"),
            key_prefix: row.get("key_prefix"),
            status: row.get("status"),
            created_at: row.get("created_at"),
        })
        .collect();
    Ok(rows)
}

/// Loads ONE pool key by id together with its owner (self-service checks).
/// Returns the masked display fields only.
pub async fn pool_key_with_owner(
    db: &Db,
    key_id: &str,
) -> sqlx::Result<Option<(PublicPoolKey, Option<String>)>> {
    let sql = db.sql(
        "SELECT k.id, k.pool_id, k.key_prefix, k.user_id, k.status, k.created_at \
         FROM pool_keys k WHERE k.id = ?",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(key_id)
        .fetch_optional(db.pool())
        .await?
        .map(|row| {
            let user_id: Option<String> = row.get("user_id");
            (
                PublicPoolKey {
                    id: row.get("id"),
                    pool_id: row.get("pool_id"),
                    key_prefix: row.get("key_prefix"),
                    user_id: user_id.clone(),
                    status: row.get("status"),
                    created_at: row.get("created_at"),
                },
                user_id,
            )
        });
    Ok(row)
}

// ---------------------------------------------------------------------------
// M4: leak prevention — key governance, review queue, alerts,
// classifier config/scores
// ---------------------------------------------------------------------------

/// A pool-key row as the SHARED listing serves it (SPEC §9 export surface).
///
/// SECURITY INVARIANT (director directive, M4 kickoff): this shape must be
/// byte-identical across rows — same five fields
/// marker, no `user_id` (nullable ownership is itself a tell), no
/// kind-dependent ordering. Ownership is visible ONLY
/// through the admin management view ([`ManagedPoolKey`]).
#[derive(Debug, Clone, Serialize)] // serde: this struct → JSON — the uniform export shape
pub struct SharedPoolKey {
    pub id: String,
    pub pool_id: String,
    pub key_prefix: String,
    pub status: String,
    pub created_at: i64,
}

/// Lists a pool's keys in the uniform export shape,
/// ordered by `created_at` — the same key for both kinds, so the order can
/// never encode what a row is (canaries are backdated into the pool's
/// existing history at plant time to interleave naturally).
pub async fn shared_pool_keys_by_pool(db: &Db, pool_id: &str) -> sqlx::Result<Vec<SharedPoolKey>> {
    let sql = db.sql(
        "SELECT id, pool_id, key_prefix, status, created_at \
         FROM pool_keys WHERE pool_id = ? ORDER BY created_at, id",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_id)
        .fetch_all(db.pool())
        .await?
        .into_iter()
        .map(|row| SharedPoolKey {
            id: row.get("id"),
            pool_id: row.get("pool_id"),
            key_prefix: row.get("key_prefix"),
            status: row.get("status"),
            created_at: row.get("created_at"),
        })
        .collect();
    Ok(rows)
}

/// A pool-key row as the ADMIN MANAGEMENT view serves it — the only place
/// ownership, expiry, and rotation lineage are exposed.
#[derive(Debug, Clone, Serialize)] // serde: this struct → JSON — admin management view
pub struct ManagedPoolKey {
    pub id: String,
    pub pool_id: String,
    pub key_prefix: String,
    pub user_id: Option<String>,
    pub status: String,
    pub expires_at: Option<i64>,
    pub created_at: i64,
}

/// Lists a pool's keys with full governance detail (admin management view).
pub async fn managed_pool_keys_by_pool(
    db: &Db,
    pool_id: &str,
) -> sqlx::Result<Vec<ManagedPoolKey>> {
    let sql = db.sql(
        "SELECT id, pool_id, key_prefix, user_id, status, expires_at, created_at \
         FROM pool_keys WHERE pool_id = ? ORDER BY created_at, id",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_id)
        .fetch_all(db.pool())
        .await?
        .into_iter()
        .map(|row| ManagedPoolKey {
            id: row.get("id"),
            pool_id: row.get("pool_id"),
            key_prefix: row.get("key_prefix"),
            user_id: row.get("user_id"),
            status: row.get("status"),
            expires_at: row.get("expires_at"),
            created_at: row.get("created_at"),
        })
        .collect();
    Ok(rows)
}

/// Marks a key ROTATED: reissue replaced it (old row keeps its history).
/// Distinct from 'revoked' so the audit trail distinguishes rotation from
/// kill-switch use.
pub async fn rotate_pool_key(db: &Db, id: &str) -> sqlx::Result<u64> {
    // 'suspended' included (H-1, final-scan round 2): the handler
    // deliberately allows rotating suspended keys (leak response —
    // suspend, rotate, reissue). A guard narrower than the precheck made
    // that a silent no-op and left the suspended row resurrectable via
    // unblock. 'rotated' is terminal: unblock only lifts 'suspended'.
    let sql = db.sql(
        "UPDATE pool_keys SET status = 'rotated' \
         WHERE id = ? AND status IN ('active', 'suspended')",
    );
    let res = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(res.rows_affected())
}

/// Atomic rotation (H-1/F1, final-scan round 2): kill the old credential,
/// mint its replacement, and carry the IP allowlist — ALL in one
/// transaction, or none of it. The status guard runs FIRST and a 0-row
/// result rolls everything back, so a key that died (revoked/rotated)
/// between request and execution can never leave an orphan `active` row
/// behind a rejected action, and an allowlist-copy failure can never
/// hand out an UNRESTRICTED replacement key.
///
/// Returns Ok(false) when the old key was no longer rotatable — callers
/// translate that into a refusal, with zero side effects persisted.
pub async fn rotate_key_atomic(db: &Db, old_id: &str, new_key: &NewPoolKey) -> sqlx::Result<bool> {
    let mut tx = db.pool().begin().await?;
    let guard = db.sql(
        "UPDATE pool_keys SET status = 'rotated' \
         WHERE id = ? AND status IN ('active', 'suspended')",
    );
    let res = sqlx::query(sqlx::AssertSqlSafe(guard.as_str()))
        .bind(old_id)
        .execute(&mut *tx)
        .await?;
    if res.rows_affected() == 0 {
        // Drop the transaction: the INSERT never runs, the guard rolls
        // back — the world is exactly as it was.
        tx.rollback().await?;
        return Ok(false);
    }
    let ins = db.sql(
        "INSERT INTO pool_keys \
         (id, pool_id, key_hash, key_prefix, kind, status, user_id, expires_at, created_at) \
         VALUES (?, ?, ?, ?, 'live', 'active', ?, ?, ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(ins.as_str()))
        .bind(&new_key.id)
        .bind(&new_key.pool_id)
        .bind(&new_key.key_hash)
        .bind(&new_key.key_prefix)
        .bind(&new_key.user_id)
        .bind(new_key.expires_at)
        .bind(new_key.created_at)
        .execute(&mut *tx)
        .await?;
    // Allowlist rows ride the same transaction (per-row app-side ids —
    // the dual-DB rule forbids engine-specific SQL like gen_random_uuid).
    let list = db.sql("SELECT cidr, note FROM ip_allowlists WHERE pool_key_id = ?");
    let rows = sqlx::query(sqlx::AssertSqlSafe(list.as_str()))
        .bind(old_id)
        .fetch_all(&mut *tx)
        .await?;
    for r in rows {
        let cidr: String = r.try_get("cidr")?;
        let note: Option<String> = r.try_get("note")?;
        let copy = db.sql(
            "INSERT INTO ip_allowlists (id, pool_key_id, cidr, note, created_at) \
             VALUES (?, ?, ?, ?, ?)",
        );
        sqlx::query(sqlx::AssertSqlSafe(copy.as_str()))
            .bind(tributary_common::db::new_id())
            .bind(&new_key.id)
            .bind(&cidr)
            .bind(&note)
            .bind(tributary_common::db::now_ms())
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(true)
}

/// Suspends a key (auto-block only): every request dies with 403 until an
/// admin one-click-unblocks. Never a ban — the member row is untouched.
pub async fn suspend_pool_key(db: &Db, id: &str) -> sqlx::Result<u64> {
    let sql =
        db.sql("UPDATE pool_keys SET status = 'suspended' WHERE id = ? AND status = 'active'");
    let res = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(res.rows_affected())
}

/// Unblocks a suspended key (the review queue's one-click action).
pub async fn unsuspend_pool_key(db: &Db, id: &str) -> sqlx::Result<u64> {
    let sql =
        db.sql("UPDATE pool_keys SET status = 'active' WHERE id = ? AND status = 'suspended'");
    let res = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(res.rows_affected())
}

/// Sets or clears a key's expiry (NULL = never expires).
pub async fn set_pool_key_expiry(db: &Db, id: &str, expires_at: Option<i64>) -> sqlx::Result<u64> {
    let sql = db.sql("UPDATE pool_keys SET expires_at = ? WHERE id = ?");
    let res = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(expires_at)
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(res.rows_affected())
}

// --- IP allowlists (SPEC §6 `ip_allowlists`) -------------------------------

/// One allowlist CIDR row.
#[derive(Debug, Clone, Serialize)] // serde: this struct → JSON — allowlist row for the admin UI
pub struct IpAllowlistRow {
    pub id: String,
    pub pool_key_id: String,
    pub cidr: String,
    pub note: Option<String>,
    pub created_at: i64,
}

/// Adds a CIDR to a key's allowlist (idempotent per the UNIQUE index —
/// callers map constraint errors to a friendly 409).
pub async fn insert_ip_allowlist(
    db: &Db,
    id: &str,
    pool_key_id: &str,
    cidr: &str,
    note: Option<&str>,
    created_at: i64,
) -> sqlx::Result<()> {
    let sql = db.sql(
        "INSERT INTO ip_allowlists (id, pool_key_id, cidr, note, created_at) \
         VALUES (?, ?, ?, ?, ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .bind(pool_key_id)
        .bind(cidr)
        .bind(note)
        .bind(created_at)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// Lists the CIDRs pinned to one key (empty = unrestricted, dormant).
/// Copies every CIDR row from one key to another (rotation contract).
/// Row-by-row on purpose: each copy needs its OWN id, and id generation
/// lives app-side (dual-DB rule — no engine-specific SQL like
/// gen_random_uuid here).
pub async fn copy_ip_allowlist(db: &Db, from_key: &str, to_key: &str) -> sqlx::Result<u64> {
    let rows = ip_allowlists_for_key(db, from_key).await?;
    let mut copied = 0u64;
    for r in rows {
        insert_ip_allowlist(
            db,
            &tributary_common::db::new_id(),
            to_key,
            &r.cidr,
            r.note.as_deref(),
            tributary_common::db::now_ms(),
        )
        .await?;
        copied += 1;
    }
    Ok(copied)
}

pub async fn ip_allowlists_for_key(
    db: &Db,
    pool_key_id: &str,
) -> sqlx::Result<Vec<IpAllowlistRow>> {
    let sql = db.sql(
        "SELECT id, pool_key_id, cidr, note, created_at FROM ip_allowlists \
         WHERE pool_key_id = ? ORDER BY created_at",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_key_id)
        .fetch_all(db.pool())
        .await?
        .into_iter()
        .map(|row| IpAllowlistRow {
            id: row.get("id"),
            pool_key_id: row.get("pool_key_id"),
            cidr: row.get("cidr"),
            note: row.get("note"),
            created_at: row.get("created_at"),
        })
        .collect();
    Ok(rows)
}

/// Lists every allowlist row for a whole pool (management view, one query).
pub async fn ip_allowlists_for_pool(db: &Db, pool_id: &str) -> sqlx::Result<Vec<IpAllowlistRow>> {
    let sql = db.sql(
        "SELECT a.id, a.pool_key_id, a.cidr, a.note, a.created_at \
         FROM ip_allowlists a JOIN pool_keys k ON a.pool_key_id = k.id \
         WHERE k.pool_id = ? ORDER BY a.created_at",
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_id)
        .fetch_all(db.pool())
        .await?
        .into_iter()
        .map(|row| IpAllowlistRow {
            id: row.get("id"),
            pool_key_id: row.get("pool_key_id"),
            cidr: row.get("cidr"),
            note: row.get("note"),
            created_at: row.get("created_at"),
        })
        .collect();
    Ok(rows)
}

/// Removes one allowlist row.
pub async fn delete_ip_allowlist(db: &Db, id: &str) -> sqlx::Result<u64> {
    let sql = db.sql("DELETE FROM ip_allowlists WHERE id = ?");
    let res = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(res.rows_affected())
}

// --- Org settings (D32 detector knobs) -----------------------------------------

/// The org-level knobs this deployment exposes. Absent row = compiled
/// defaults, so most orgs never touch this table.
#[derive(Debug, Clone, Serialize)] // serde: this struct → JSON — admin settings view
pub struct OrgSettings {
    /// Distinct-IP concurrency window in seconds (D32). 300 (5 min) by
    /// default; clamped at the API on write.
    pub concurrency_window_secs: i64,
    pub updated_at: i64,
}

/// The compiled default window — also what `org_settings` returns when the
/// org has never tuned anything (no row is created for reads).
pub const DEFAULT_CONCURRENCY_WINDOW_SECS: i64 = 300;

/// Reads an org's settings, falling back to defaults without inserting.
pub async fn org_settings(db: &Db, org_id: &str) -> sqlx::Result<OrgSettings> {
    let sql =
        db.sql("SELECT concurrency_window_secs, updated_at FROM org_settings WHERE org_id = ?");
    match sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(org_id)
        .fetch_optional(db.pool())
        .await?
    {
        Some(row) => Ok(OrgSettings {
            // The DEFAULT clause guarantees non-NULL; the fallback keeps
            // honest even against a manually NULLed row.
            concurrency_window_secs: row
                .try_get("concurrency_window_secs")
                .unwrap_or(DEFAULT_CONCURRENCY_WINDOW_SECS),
            updated_at: row.get("updated_at"),
        }),
        None => Ok(OrgSettings {
            concurrency_window_secs: DEFAULT_CONCURRENCY_WINDOW_SECS,
            updated_at: 0,
        }),
    }
}

/// Upserts the knobs (validated at the API layer; values arrive as i64 per
/// the dual-DB INTEGER rule).
pub async fn upsert_org_settings(
    db: &Db,
    org_id: &str,
    concurrency_window_secs: i64,
    updated_at: i64,
) -> sqlx::Result<()> {
    let sql = db.sql(
        "INSERT INTO org_settings (org_id, concurrency_window_secs, updated_at) \
         VALUES (?, ?, ?) \
         ON CONFLICT (org_id) DO UPDATE SET \
             concurrency_window_secs = excluded.concurrency_window_secs, \
             updated_at = excluded.updated_at",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(org_id)
        .bind(concurrency_window_secs)
        .bind(updated_at)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// How many distinct-IP concurrency flags this key already has inside the
/// window (D32 de-duplication: one alert per key per window, not one per
/// request — a shared key can do hundreds of requests in 5 minutes).
pub async fn concurrency_flags_for_key(
    db: &Db,
    pool_key_id: &str,
    since_ms: i64,
) -> sqlx::Result<i64> {
    let sql = db.sql(
        "SELECT COUNT(*) AS n FROM review_flags \
         WHERE pool_key_id = ? AND kind = 'distinct_ip_concurrency' AND created_at >= ?",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_key_id)
        .bind(since_ms)
        .fetch_one(db.pool())
        .await?;
    let n: i64 = row.get("n");
    Ok(n)
}

// --- Pending actions (D33 admin four-eyes) -------------------------------------

/// One deferred sensitive action. The payload is SANITIZED parameters
/// (sealed blobs for secrets — never plaintext keys, rule 2); the write
/// only happens when a second, distinct admin approves.
// No Serialize on purpose (final-scan F7): this row carries the sealed
// payload; an accidental Json(row) must not compile into a leak.
#[derive(Debug, Clone)]
pub struct PendingActionRow {
    pub id: String,
    pub org_id: String,
    pub kind: String,
    /// The action's subject (admin id / key id / pool id / org id) — for
    /// display and re-validation at execution time.
    pub target_id: String,
    pub payload_json: String,
    pub requested_by: String,
    pub status: String,
    pub decided_by: Option<String>,
    pub decided_at: Option<i64>,
    pub created_at: i64,
}

/// The masked public view served to admins. `summary` is the payload's
/// sanitized one-liner (built server-side — the raw payload, which may
/// hold sealed blobs, never crosses the wire).
#[derive(Debug, Clone, Serialize)] // serde: this struct → JSON — queue view
pub struct PublicPendingAction {
    pub id: String,
    pub org_id: String,
    pub kind: String,
    pub target_id: String,
    pub requested_by: String,
    pub status: String,
    pub created_at: i64,
    pub decided_by: Option<String>,
    pub decided_at: Option<i64>,
    pub summary: String,
}

/// The email of a pool key's OWNER user (read-time enrichment for D33
/// approval views: legacy pending rows whose payload predates the
/// owner_email field still resolve the affected admin).
pub async fn key_owner_email(db: &Db, pool_key_id: &str) -> sqlx::Result<Option<String>> {
    let sql = db.sql(
        "SELECT u.email AS email FROM pool_keys k \
         JOIN users u ON u.id = k.user_id WHERE k.id = ?",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_key_id)
        .fetch_optional(db.pool())
        .await?;
    Ok(row.map(|r| r.get::<String, _>("email")))
}

/// How many org_admins the org has (D33 gate: ≥2 ⇒ dual-control).
pub async fn org_admin_count(db: &Db, org_id: &str) -> sqlx::Result<i64> {
    let sql = db.sql("SELECT COUNT(*) AS n FROM users WHERE org_id = ? AND role = 'org_admin'");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(org_id)
        .fetch_one(db.pool())
        .await?;
    let n: i64 = row.get("n");
    Ok(n)
}

/// Inserts one pending action (status='pending').
#[allow(clippy::too_many_arguments)]
pub async fn insert_pending_action(
    db: &Db,
    id: &str,
    org_id: &str,
    kind: &str,
    target_id: &str,
    payload_json: &str,
    requested_by: &str,
    created_at: i64,
) -> sqlx::Result<()> {
    let sql = db.sql(
        "INSERT INTO pending_actions \
         (id, org_id, kind, target_id, payload_json, requested_by, status, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, 'pending', ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .bind(org_id)
        .bind(kind)
        .bind(target_id)
        .bind(payload_json)
        .bind(requested_by)
        .bind(created_at)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// Maps one sqlx ANY row into a PendingActionRow (shared by both list
/// and by-id queries — column order mirrors the SELECTs above).
fn pending_action_from_row(row: sqlx::any::AnyRow) -> PendingActionRow {
    PendingActionRow {
        id: row.get("id"),
        org_id: row.get("org_id"),
        kind: row.get("kind"),
        target_id: row.get("target_id"),
        payload_json: row.get("payload_json"),
        requested_by: row.get("requested_by"),
        status: row.get("status"),
        decided_by: row.get("decided_by"),
        decided_at: row.get("decided_at"),
        created_at: row.get("created_at"),
    }
}

/// Lists an org's pending actions, newest first (status filter optional).
pub async fn pending_actions_by_org(
    db: &Db,
    org_id: &str,
    status: Option<&str>,
) -> sqlx::Result<Vec<PendingActionRow>> {
    let (sql, has_status) = match status {
        Some(_) => (
            db.sql(
                "SELECT id, org_id, kind, target_id, payload_json, requested_by, status, \
                  decided_by, decided_at, created_at FROM pending_actions \
                 WHERE org_id = ? AND status = ? ORDER BY created_at DESC",
            ),
            true,
        ),
        None => (
            db.sql(
                "SELECT id, org_id, kind, target_id, payload_json, requested_by, status, \
                  decided_by, decided_at, created_at FROM pending_actions \
                 WHERE org_id = ? ORDER BY created_at DESC",
            ),
            false,
        ),
    };
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.as_str())).bind(org_id);
    if has_status {
        q = q.bind(status.unwrap());
    }
    let rows = q.fetch_all(db.pool()).await?;
    Ok(rows
        .iter()
        .map(|r| pending_action_from_row(r.clone()))
        .collect())
}

/// Loads one pending action scoped to the org (None = wrong org or gone).
pub async fn pending_action_by_id(
    db: &Db,
    org_id: &str,
    id: &str,
) -> sqlx::Result<Option<PendingActionRow>> {
    let sql = db.sql(
        "SELECT id, org_id, kind, target_id, payload_json, requested_by, status, \
          decided_by, decided_at, created_at FROM pending_actions \
         WHERE org_id = ? AND id = ?",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(org_id)
        .bind(id)
        .fetch_optional(db.pool())
        .await?;
    Ok(row.map(pending_action_from_row))
}

/// Marks a decision (executed | rejected). Returns rows affected (0 =
/// already decided — the caller treats that as conflict).
pub async fn decide_pending_action(
    db: &Db,
    id: &str,
    status: &str,
    decided_by: &str,
    decided_at: i64,
) -> sqlx::Result<u64> {
    let sql = db.sql(
        "UPDATE pending_actions SET status = ?, decided_by = ?, decided_at = ? \
         WHERE id = ? AND status = 'pending'",
    );
    let res = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(status)
        .bind(decided_by)
        .bind(decided_at)
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(res.rows_affected())
}

/// The most recent OPEN flag linked to a key (D33 containment link).
pub async fn latest_open_flag_for_key(db: &Db, pool_key_id: &str) -> sqlx::Result<Option<String>> {
    let sql = db.sql(
        "SELECT id FROM review_flags WHERE pool_key_id = ? AND status = 'open' \
         ORDER BY created_at DESC LIMIT 1",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_key_id)
        .fetch_optional(db.pool())
        .await?;
    Ok(row.map(|r| r.get::<String, _>("id")))
}

/// Claims a pending action for execution (final-scan F2: the approver
/// wins the row BEFORE any side effect — concurrent approvers can no
/// longer both execute, and a guard failure can no longer overwrite a
/// winner's trail with 'rejected'). 0 rows = someone else decided first.
pub async fn claim_pending_action(
    db: &Db,
    id: &str,
    decided_by: &str,
    decided_at: i64,
) -> sqlx::Result<u64> {
    let sql = db.sql(
        "UPDATE pending_actions SET status = 'executing', decided_by = ?, decided_at = ? \
         WHERE id = ? AND status = 'pending'",
    );
    let res = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(decided_by)
        .bind(decided_at)
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(res.rows_affected())
}

/// Finalizes a claimed ('executing') action to executed|rejected. A crash
/// between claim and finalize strands the row in 'executing' (visible,
/// never re-executable — strictly safer than the old crash-after-execute
/// which left a re-executable 'pending').
pub async fn finalize_pending_action(
    db: &Db,
    id: &str,
    status: &str,
    decided_at: i64,
) -> sqlx::Result<u64> {
    let sql = db.sql(
        "UPDATE pending_actions SET status = ?, decided_at = ? \
         WHERE id = ? AND status = 'executing'",
    );
    let res = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(status)
        .bind(decided_at)
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(res.rows_affected())
}

/// Demotes an org_admin to member ATOMICALLY (final-scan TOCTOU fix).
///
/// The ≥2-admins guard lives INSIDE the UPDATE so a stale count can never
/// authorize a demote — but on Postgres MVCC two concurrent demotes could
/// each read count=2 in their statement snapshots and BOTH win, leaving
/// the org admin-less. The leading SELECT closes that window: on PG it
/// takes row locks on the whole admin bench, so the second transaction
/// blocks until the first commits and then sees count=1 and refuses.
/// SQLite serializes writers natively (one write lock) and has no
/// `FOR UPDATE` — a plain read inside the transaction is equivalent.
pub async fn demote_org_admin_guarded(db: &Db, org_id: &str, user_id: &str) -> sqlx::Result<u64> {
    let mut tx = db.pool().begin().await?;
    // ORDER BY id (round-2 deadlock fix): every contender locks the
    // bench in the SAME row order. Without it, two racing demotes could
    // lock the same rows in plan-dependent opposite orders — a textbook
    // deadlock cycle; PG then killed one transaction with a 500 instead
    // of letting the guard refuse it cleanly.
    let lock = if db.postgres() {
        db.sql(
            "SELECT id FROM users WHERE org_id = ? AND role = 'org_admin' \
             ORDER BY id FOR UPDATE",
        )
    } else {
        db.sql(
            "SELECT id FROM users WHERE org_id = ? AND role = 'org_admin' \
             ORDER BY id",
        )
    };
    sqlx::query(sqlx::AssertSqlSafe(lock.as_str()))
        .bind(org_id)
        .fetch_all(&mut *tx)
        .await?;
    let sql = db.sql(
        "UPDATE users SET role = 'member' \
         WHERE id = ? AND role = 'org_admin' \
           AND (SELECT COUNT(*) FROM users \
                WHERE org_id = ? AND role = 'org_admin') >= 2",
    );
    let res = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(user_id)
        .bind(org_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(res.rows_affected())
}

/// Open review flags linked to one pool key (D33 containment check: an
/// admin revoking/expiring a key WITH a linked alert may act solo).
pub async fn open_flags_for_key(db: &Db, pool_key_id: &str) -> sqlx::Result<i64> {
    let sql = db.sql(
        "SELECT COUNT(*) AS n FROM review_flags \
         WHERE pool_key_id = ? AND status = 'open'",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(pool_key_id)
        .fetch_one(db.pool())
        .await?;
    let n: i64 = row.get("n");
    Ok(n)
}

// --- Review queue --------------------------------------------------------------

/// One review-queue flag as served to admins (detail already sanitized at
/// WRITE time — this struct cannot carry prompts or key material).
#[derive(Debug, Clone, Serialize)] // serde: this struct → JSON — review queue row
pub struct ReviewFlagRow {
    pub id: String,
    pub org_id: String,
    pub pool_id: Option<String>,
    pub pool_key_id: Option<String>,
    pub user_id: Option<String>,
    pub kind: String,
    pub severity: String,
    /// Classifier alignment in basis points (0–10000); NULL for
    /// non-scored kinds.
    pub score_bp: Option<i64>,
    pub detail_json: Value,
    pub status: String,
    pub created_at: i64,
    pub resolved_at: Option<i64>,
    pub resolved_by: Option<String>,
}

/// Parameters for [`insert_review_flag`] — one struct instead of an
/// 11-argument flat list (readable at call sites, clippy-agreed).
pub struct NewReviewFlag<'a> {
    pub id: &'a str,
    pub org_id: &'a str,
    pub pool_id: Option<&'a str>,
    pub pool_key_id: Option<&'a str>,
    pub user_id: Option<&'a str>,
    pub kind: &'a str,
    pub severity: &'a str,
    pub score_bp: Option<i64>,
    pub detail_json: &'a Value,
    pub created_at: i64,
}

/// Writes one review flag (the detection pipeline's queue insert).
pub async fn insert_review_flag(db: &Db, flag: &NewReviewFlag<'_>) -> sqlx::Result<()> {
    let sql = db.sql(
        "INSERT INTO review_flags \
         (id, org_id, pool_id, pool_key_id, user_id, kind, severity, score_bp, detail_json, \
          status, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'open', ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(flag.id)
        .bind(flag.org_id)
        .bind(flag.pool_id)
        .bind(flag.pool_key_id)
        .bind(flag.user_id)
        .bind(flag.kind)
        .bind(flag.severity)
        .bind(flag.score_bp)
        .bind(flag.detail_json.to_string())
        .bind(flag.created_at)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// Lists review flags newest-first, optionally filtered by org and status.
pub async fn list_review_flags(
    db: &Db,
    org_scope: Option<&str>,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> sqlx::Result<Vec<ReviewFlagRow>> {
    // Clauses are assembled BEFORE the portable `?`-rewrite so bind order
    // still matches placeholder order (same pattern as usage_events).
    let mut sql = String::from(
        "SELECT id, org_id, pool_id, pool_key_id, user_id, kind, severity, score_bp, \
          detail_json, status, created_at, resolved_at, resolved_by FROM review_flags",
    );
    let mut clauses: Vec<&'static str> = Vec::new();
    if org_scope.is_some() {
        clauses.push("org_id = ?");
    }
    if status.is_some() {
        clauses.push("status = ?");
    }
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    sql.push_str(" ORDER BY created_at DESC, id LIMIT ? OFFSET ?");
    let sql = db.sql(&sql);
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()));
    if let Some(org) = org_scope {
        q = q.bind(org);
    }
    if let Some(st) = status {
        q = q.bind(st);
    }
    let rows = q
        .bind(limit)
        .bind(offset)
        .fetch_all(db.pool())
        .await?
        .into_iter()
        .map(review_flag_from_row)
        .collect();
    Ok(rows)
}

/// Fetches one review flag (for scoped resolution).
pub async fn review_flag_by_id(db: &Db, id: &str) -> sqlx::Result<Option<ReviewFlagRow>> {
    let sql = db.sql(
        "SELECT id, org_id, pool_id, pool_key_id, user_id, kind, severity, score_bp, \
          detail_json, status, created_at, resolved_at, resolved_by \
         FROM review_flags WHERE id = ?",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .fetch_optional(db.pool())
        .await?
        .map(review_flag_from_row);
    Ok(row)
}

/// Maps a SELECT row into a [`ReviewFlagRow`] (corrupt detail degrades to
/// JSON null rather than 500-ing the viewer — same posture as audit rows).
fn review_flag_from_row(row: sqlx::any::AnyRow) -> ReviewFlagRow {
    let detail_raw: Option<String> = row.get("detail_json");
    ReviewFlagRow {
        id: row.get("id"),
        org_id: row.get("org_id"),
        pool_id: row.get("pool_id"),
        pool_key_id: row.get("pool_key_id"),
        user_id: row.get("user_id"),
        kind: row.get("kind"),
        severity: row.get("severity"),
        score_bp: row.get("score_bp"),
        detail_json: serde_json::from_str(&detail_raw.unwrap_or_default()).unwrap_or(Value::Null),
        status: row.get("status"),
        created_at: row.get("created_at"),
        resolved_at: row.get("resolved_at"),
        resolved_by: row.get("resolved_by"),
    }
}

/// Marks a flag resolved, recording who decided.
pub async fn resolve_review_flag(
    db: &Db,
    id: &str,
    resolved_by: &str,
    resolved_at: i64,
) -> sqlx::Result<u64> {
    let sql = db.sql(
        "UPDATE review_flags SET status = 'resolved', resolved_by = ?, resolved_at = ? \
         WHERE id = ? AND status = 'open'",
    );
    let res = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(resolved_by)
        .bind(resolved_at)
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(res.rows_affected())
}

// --- Email alert outbox ---------------------------------------------------------

/// One outbox row (assertable even on the SMTP-unconfigured fallback path).
#[derive(Debug, Clone, Serialize)] // serde: this struct → JSON — ops/debug view of the outbox
pub struct EmailAlertRow {
    pub id: String,
    pub org_id: String,
    pub to_email: String,
    pub subject: String,
    pub body: String,
    pub status: String,
    pub created_at: i64,
    pub sent_at: Option<i64>,
}

/// Enqueues an alert email (send happens async; failure never blocks the
/// request that raised the alert).
pub async fn insert_email_alert(
    db: &Db,
    id: &str,
    org_id: &str,
    to_email: &str,
    subject: &str,
    body: &str,
    created_at: i64,
) -> sqlx::Result<()> {
    let sql = db.sql(
        "INSERT INTO email_alerts (id, org_id, to_email, subject, body, status, created_at) \
         VALUES (?, ?, ?, ?, ?, 'queued', ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .bind(org_id)
        .bind(to_email)
        .bind(subject)
        .bind(body)
        .bind(created_at)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// Updates an outbox row after a send attempt ('sent' or 'skipped').
pub async fn set_email_alert_status(
    db: &Db,
    id: &str,
    status: &str,
    sent_at: Option<i64>,
) -> sqlx::Result<u64> {
    let sql = db.sql("UPDATE email_alerts SET status = ?, sent_at = ? WHERE id = ?");
    let res = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(status)
        .bind(sent_at)
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(res.rows_affected())
}

/// Reads one outbox row (tests assert the alert fired).
pub async fn email_alert_by_id(db: &Db, id: &str) -> sqlx::Result<Option<EmailAlertRow>> {
    let sql = db.sql(
        "SELECT id, org_id, to_email, subject, body, status, created_at, sent_at \
         FROM email_alerts WHERE id = ?",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .fetch_optional(db.pool())
        .await?
        .map(|row| EmailAlertRow {
            id: row.get("id"),
            org_id: row.get("org_id"),
            to_email: row.get("to_email"),
            subject: row.get("subject"),
            body: row.get("body"),
            status: row.get("status"),
            created_at: row.get("created_at"),
            sent_at: row.get("sent_at"),
        });
    Ok(row)
}

/// The org's alert recipients: its org_admins, or the host admins when the
/// org has none of its own (host-managed orgs still deserve the alert).
pub async fn alert_recipients(db: &Db, org_id: &str) -> sqlx::Result<Vec<String>> {
    let admins = db
        .sql("SELECT email FROM users WHERE org_id = ? AND role = 'org_admin' ORDER BY created_at");
    let rows = sqlx::query(sqlx::AssertSqlSafe(admins.as_str()))
        .bind(org_id)
        .fetch_all(db.pool())
        .await?;
    let mut emails: Vec<String> = rows.iter().map(|r| r.get("email")).collect();
    if emails.is_empty() {
        let hosts = db.sql("SELECT email FROM users WHERE role = 'host_admin' ORDER BY created_at");
        let rows = sqlx::query(sqlx::AssertSqlSafe(hosts.as_str()))
            .fetch_all(db.pool())
            .await?;
        emails = rows.iter().map(|r| r.get("email")).collect();
    }
    Ok(emails)
}

// --- Classifier config (D28) -----------------------------------------------------

/// A full `classifier_config` row — internal use only (the API serves the
/// masked view without ciphertext).
#[derive(Debug, Clone)]
pub struct ClassifierConfigRow {
    pub org_id: String,
    pub base_url: String,
    /// Wire contract: "jev" (TypeSafe systemone) or "custom" (generic).
    pub dialect: String,
    pub model: String,
    pub ciphertext: String,
    pub dek_wrapped: String,
    pub key_prefix: String,
    pub purpose_statement: String,
    pub mode: String,
    pub alert_threshold_bp: i64,
    pub block_threshold_bp: i64,
    pub auto_block_live: bool,
    pub shadow_started_at: Option<i64>,
    pub sampling_per_hour: i64,
    pub updated_at: i64,
}

/// The masked, API-facing classifier view — no ciphertext, plus the shadow
/// statistics the settings page renders ("would have blocked N from M").
#[derive(Debug, Clone, Serialize)] // serde: this struct → JSON — classifier settings view
pub struct PublicClassifierConfig {
    pub org_id: String,
    pub base_url: String,
    /// Wire contract: "jev" | "custom" (see classifier::Dialect).
    pub dialect: String,
    pub model: String,
    pub key_prefix: String,
    pub purpose_statement: String,
    pub mode: String,
    pub alert_threshold: f64,
    pub auto_block_threshold: f64,
    pub auto_block_live: bool,
    pub shadow_started_at: Option<i64>,
    pub sampling_per_hour: i64,
    pub updated_at: i64,
    /// Count of sampled requests under the auto-block threshold since the
    /// shadow window began (the "would have blocked N" number).
    pub shadow_would_have_blocked: i64,
    /// Distinct members those samples came from (the "from M members").
    pub shadow_distinct_members: i64,
}

/// Maps a raw row into the public view, computing shadow stats from the
/// scores table (one query; the numbers are small at M4 scale).
pub async fn public_classifier_config(
    db: &Db,
    row: &ClassifierConfigRow,
) -> sqlx::Result<PublicClassifierConfig> {
    let since = row.shadow_started_at.unwrap_or(0);
    let stats = db.sql(
        "SELECT COUNT(*) AS n, COUNT(DISTINCT COALESCE(user_id, pool_key_id)) AS m \
         FROM classifier_scores WHERE org_id = ? AND would_block = 1 AND ts >= ?",
    );
    let stat_row = sqlx::query(sqlx::AssertSqlSafe(stats.as_str()))
        .bind(&row.org_id)
        .bind(since)
        .fetch_one(db.pool())
        .await?;
    Ok(PublicClassifierConfig {
        org_id: row.org_id.clone(),
        base_url: row.base_url.clone(),
        dialect: row.dialect.clone(),
        model: row.model.clone(),
        key_prefix: row.key_prefix.clone(),
        purpose_statement: row.purpose_statement.clone(),
        mode: row.mode.clone(),
        alert_threshold: row.alert_threshold_bp as f64 / 10_000.0,
        auto_block_threshold: row.block_threshold_bp as f64 / 10_000.0,
        auto_block_live: row.auto_block_live,
        shadow_started_at: row.shadow_started_at,
        sampling_per_hour: row.sampling_per_hour,
        updated_at: row.updated_at,
        shadow_would_have_blocked: stat_row.get("n"),
        shadow_distinct_members: stat_row.get("m"),
    })
}

/// Loads an org's classifier config (None ⇒ feature dormant, D28).
pub async fn classifier_config_by_org(
    db: &Db,
    org_id: &str,
) -> sqlx::Result<Option<ClassifierConfigRow>> {
    let sql = db.sql(
        "SELECT org_id, base_url, dialect, model, ciphertext, dek_wrapped, key_prefix, \
          purpose_statement, mode, alert_threshold_bp, block_threshold_bp, \
          auto_block_live, shadow_started_at, sampling_per_hour, updated_at \
         FROM classifier_config WHERE org_id = ?",
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(org_id)
        .fetch_optional(db.pool())
        .await?
        .map(classifier_row_from_row);
    Ok(row)
}

/// Upserts (replace via delete+insert in one tx, mirroring quota rules —
/// avoids ON CONFLICT dialect differences) an org's classifier config.
pub async fn upsert_classifier_config(db: &Db, row: &ClassifierConfigRow) -> sqlx::Result<()> {
    let mut tx = db.pool().begin().await?;
    let del = db.sql("DELETE FROM classifier_config WHERE org_id = ?");
    sqlx::query(sqlx::AssertSqlSafe(del.as_str()))
        .bind(&row.org_id)
        .execute(&mut *tx)
        .await?;
    let ins = db.sql(
        "INSERT INTO classifier_config \
         (org_id, base_url, dialect, model, ciphertext, dek_wrapped, key_prefix, purpose_statement, \
          mode, alert_threshold_bp, block_threshold_bp, auto_block_live, shadow_started_at, \
          sampling_per_hour, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(ins.as_str()))
        .bind(&row.org_id)
        .bind(&row.base_url)
        .bind(&row.dialect)
        .bind(&row.model)
        .bind(&row.ciphertext)
        .bind(&row.dek_wrapped)
        .bind(&row.key_prefix)
        .bind(&row.purpose_statement)
        .bind(&row.mode)
        .bind(row.alert_threshold_bp)
        .bind(row.block_threshold_bp)
        // BIGINT column: bind the integer form — sqlx maps a raw bool to
        // BOOLEAN, which Postgres rejects for a BIGINT target (SQLite's
        // loose typing hides this; rule: dual-DB means INTEGER 0/1 only).
        .bind(row.auto_block_live as i64)
        .bind(row.shadow_started_at)
        .bind(row.sampling_per_hour)
        .bind(row.updated_at)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Deletes an org's classifier config — the feature goes dormant again.
pub async fn delete_classifier_config(db: &Db, org_id: &str) -> sqlx::Result<u64> {
    let sql = db.sql("DELETE FROM classifier_config WHERE org_id = ?");
    let res = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(org_id)
        .execute(db.pool())
        .await?;
    Ok(res.rows_affected())
}

/// Flips auto-block from shadow to live (admin decision after reviewing
/// the would-have-blocked stats). Returns rows affected (0 = no config,
/// wrong mode, or already live).
pub async fn classifier_go_live(db: &Db, org_id: &str, at: i64) -> sqlx::Result<u64> {
    let sql = db.sql(
        "UPDATE classifier_config SET auto_block_live = 1, updated_at = ? \
         WHERE org_id = ? AND mode = 'auto_block' AND auto_block_live = 0",
    );
    let res = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(at)
        .bind(org_id)
        .execute(db.pool())
        .await?;
    Ok(res.rows_affected())
}

/// Maps a SELECT row into a [`ClassifierConfigRow`].
fn classifier_row_from_row(row: sqlx::any::AnyRow) -> ClassifierConfigRow {
    let live: i64 = row.get("auto_block_live");
    ClassifierConfigRow {
        org_id: row.get("org_id"),
        base_url: row.get("base_url"),
        dialect: row.get("dialect"),
        model: row.get("model"),
        ciphertext: row.get("ciphertext"),
        dek_wrapped: row.get("dek_wrapped"),
        key_prefix: row.get("key_prefix"),
        purpose_statement: row.get("purpose_statement"),
        mode: row.get("mode"),
        alert_threshold_bp: row.get("alert_threshold_bp"),
        block_threshold_bp: row.get("block_threshold_bp"),
        auto_block_live: live != 0,
        shadow_started_at: row.get("shadow_started_at"),
        sampling_per_hour: row.get("sampling_per_hour"),
        updated_at: row.get("updated_at"),
    }
}

// --- Classifier scores --------------------------------------------------------

/// Parameters for [`insert_classifier_score`] (same shape rationale as
/// [`NewReviewFlag`]).
pub struct NewClassifierScore<'a> {
    pub id: &'a str,
    pub org_id: &'a str,
    pub pool_key_id: Option<&'a str>,
    pub user_id: Option<&'a str>,
    pub model: &'a str,
    pub score_bp: i64,
    pub would_block: bool,
    pub ts: i64,
}

/// Writes one sampled classification (score + verdict metadata, NEVER the
/// prompt — D28).
pub async fn insert_classifier_score(db: &Db, score: &NewClassifierScore<'_>) -> sqlx::Result<()> {
    let sql = db.sql(
        "INSERT INTO classifier_scores \
         (id, org_id, pool_key_id, user_id, model, score_bp, would_block, ts) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(score.id)
        .bind(score.org_id)
        .bind(score.pool_key_id)
        .bind(score.user_id)
        .bind(score.model)
        .bind(score.score_bp)
        .bind(score.would_block as i64)
        .bind(score.ts)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// Count of samples recorded for the org in the current hour — the
/// sampling budget gate (N/hour, D28).
pub async fn classifier_samples_this_hour(
    db: &Db,
    org_id: &str,
    hour_start_ms: i64,
) -> sqlx::Result<i64> {
    let sql = db.sql("SELECT COUNT(*) AS n FROM classifier_scores WHERE org_id = ? AND ts >= ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(org_id)
        .bind(hour_start_ms)
        .fetch_one(db.pool())
        .await?;
    let n: i64 = row.get("n");
    Ok(n)
}
