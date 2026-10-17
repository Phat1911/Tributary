//! Control-plane queries (portable `?`-SQL via `tributary_common::db::Db`).
//!
//! Row types that cross into responses are split: full rows (for internal
//! use, e.g. decrypting a provider key) vs. the API's masked views — the
//! handlers never serialize `password_hash` or ciphertext columns.

use serde::Serialize;
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
        "SELECT s.id AS session_id, s.expires_at, u.id AS user_id, u.role, u.org_id \
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
    pub created_at: i64,
}

/// Inserts an issued pool key.
pub async fn insert_issued_pool_key(db: &Db, key: &NewPoolKey) -> sqlx::Result<()> {
    let sql = db.sql(
        "INSERT INTO pool_keys (id, pool_id, key_hash, key_prefix, kind, status, user_id, created_at) \
         VALUES (?, ?, ?, ?, 'live', 'active', ?, ?)",
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(&key.id)
        .bind(&key.pool_id)
        .bind(&key.key_hash)
        .bind(&key.key_prefix)
        .bind(&key.user_id)
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
pub async fn revoke_pool_key(db: &Db, id: &str) -> sqlx::Result<u64> {
    let sql = db.sql("UPDATE pool_keys SET status = 'revoked' WHERE id = ? AND status = 'active'");
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
