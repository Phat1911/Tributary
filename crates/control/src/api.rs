//! `/api/v1` handlers (SPEC §7 control-plane subset).
//!
//! Conventions: request bodies are typed serde structs (rejecting unknown
//! shapes early); responses never serialize `password_hash`, raw keys, or
//! ciphertext columns. Raw secrets (`key` fields) are moved into
//! `Zeroizing` immediately after parsing so their bytes are scrubbed on
//! drop — plaintext lifetime is one handler long.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde::Serialize;
use zeroize::Zeroizing;

use crate::ControlState;
use crate::auth::{self, AuthedAdmin};
use crate::db::{self, PublicPoolKey, PublicProviderKey, PublicUser};
use crate::error::{ApiResult, ControlError};
use tributary_common::db::{new_id, now_ms};
use tributary_common::keys::generate_key;
use tributary_crypto::{MasterKey, row_ad, seal};

// ---------------------------------------------------------------------------
// Auth
// ---------------------------------------------------------------------------

/// POST /api/v1/auth/login body.
#[derive(Deserialize)] // serde: JSON → this struct, for request parsing
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// POST /api/v1/auth/login response — the raw session token, shown once.
#[derive(Serialize)] // serde: this struct → JSON, for response rendering
pub struct LoginResponse {
    pub token: String,
    pub expires_at: i64,
}

/// Verifies credentials and mints a session token.
pub async fn login(
    State(state): State<ControlState>,
    Json(body): Json<LoginRequest>,
) -> ApiResult<Json<LoginResponse>> {
    let email = body.email.trim().to_lowercase();
    // One generic 401 for unknown email AND wrong password (no probing).
    let user = db::user_by_email(&state.db, &email)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::Unauthorized)?;
    if !auth::verify_password(&user.password_hash, &body.password) {
        return Err(ControlError::Unauthorized);
    }
    let (raw, hash, expires_at) = auth::generate_session_token();
    db::insert_session(&state.db, &new_id(), &user.id, &hash, now_ms(), expires_at)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(LoginResponse {
        token: raw,
        expires_at,
    }))
}

/// POST /api/v1/auth/logout — deletes the caller's session.
pub async fn logout(
    State(state): State<ControlState>,
    headers: axum::http::HeaderMap,
    _admin: AuthedAdmin,
) -> ApiResult<StatusCode> {
    // AuthedAdmin already proved the session is valid; now resolve it again
    // by the presented token to learn WHICH session row to delete.
    if let Some(token) = crate::auth::bearer_token(&headers) {
        let hash = tributary_common::keys::hash_token(token);
        if let Ok(Some(row)) = db::session_user_by_token_hash(&state.db, &hash).await {
            let _ = db::delete_session(&state.db, &row.session_id).await;
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Orgs
// ---------------------------------------------------------------------------

/// POST /api/v1/orgs body.
#[derive(Deserialize)]
pub struct CreateOrgRequest {
    pub name: String,
    pub slug: String,
}

/// The org view served by the API.
#[derive(Serialize)]
pub struct PublicOrg {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub created_at: i64,
}

/// Creates an organization (host_admin anywhere; org_admin nowhere — orgs
/// are provisioned by the host in M2).
pub async fn create_org(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Json(body): Json<CreateOrgRequest>,
) -> ApiResult<(StatusCode, Json<PublicOrg>)> {
    if admin.role != "host_admin" {
        return Err(ControlError::Forbidden);
    }
    let name = body.name.trim();
    let slug = body.slug.trim().to_lowercase();
    if name.is_empty() || name.len() > 200 {
        return Err(ControlError::BadRequest(
            "name must be 1-200 characters".into(),
        ));
    }
    let valid_slug = !slug.is_empty()
        && slug.len() <= 64
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid_slug {
        return Err(ControlError::BadRequest(
            "slug must be lowercase alphanumerics and dashes".into(),
        ));
    }
    let org = db::OrgRow {
        id: new_id(),
        name: name.to_string(),
        slug: slug.clone(),
        settings: "{}".to_string(),
        created_at: now_ms(),
    };
    db::insert_org(&state.db, &org)
        .await
        .map_err(|e| ControlError::Conflict(format!("slug '{slug}' is already taken: {e}")))?;
    Ok((
        StatusCode::CREATED,
        Json(PublicOrg {
            id: org.id,
            name: org.name,
            slug: org.slug,
            created_at: org.created_at,
        }),
    ))
}

/// Lists orgs within the caller's scope.
pub async fn list_orgs(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
) -> ApiResult<Json<Vec<PublicOrg>>> {
    let orgs = db::list_orgs(&state.db, &admin.role, admin.org_id.as_deref())
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(
        orgs.iter()
            .map(|o| PublicOrg {
                id: o.id.clone(),
                name: o.name.clone(),
                slug: o.slug.clone(),
                created_at: o.created_at,
            })
            .collect(),
    ))
}

/// Fetches one org (scoped).
pub async fn get_org(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path(id): Path<String>,
) -> ApiResult<Json<PublicOrg>> {
    let org = db::org_by_id(&state.db, &id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    admin.ensure_org_scope(&org.id)?;
    Ok(Json(PublicOrg {
        id: org.id,
        name: org.name,
        slug: org.slug,
        created_at: org.created_at,
    }))
}

/// Deletes an org — only when it has no pools left (pools carry usage history).
pub async fn delete_org(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let org = db::org_by_id(&state.db, &id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    admin.ensure_org_scope(&org.id)?;
    if db::count_pools_for_org(&state.db, &org.id)
        .await
        .unwrap_or(0)
        > 0
    {
        return Err(ControlError::Conflict(
            "org still has pools — disable and migrate them first".into(),
        ));
    }
    db::delete_org(&state.db, &org.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Members
// ---------------------------------------------------------------------------

/// POST /api/v1/members body.
#[derive(Deserialize)]
pub struct CreateMemberRequest {
    pub org_id: String,
    pub email: String,
    pub password: String,
}

/// GET /api/v1/members?org_id=… filter.
#[derive(Deserialize)]
pub struct MemberListQuery {
    pub org_id: String,
}

/// Creates a member user inside an org (members authenticate to the data
/// plane only; control-plane self-service arrives M3).
pub async fn create_member(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Json(body): Json<CreateMemberRequest>,
) -> ApiResult<(StatusCode, Json<PublicUser>)> {
    admin.ensure_org_scope(&body.org_id)?;
    auth::validate_credentials(&body.email, &body.password)?;
    let member = db::UserRow {
        id: new_id(),
        email: body.email.trim().to_lowercase(),
        password_hash: auth::hash_password(&body.password)?,
        role: "member".to_string(),
        org_id: Some(body.org_id),
        created_at: now_ms(),
    };
    db::insert_user(&state.db, &member)
        .await
        .map_err(|e| ControlError::Conflict(format!("email already exists: {e}")))?;
    Ok((StatusCode::CREATED, Json(PublicUser::from(&member))))
}

/// Lists an org's members (scoped by query param).
pub async fn list_members(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Query(query): Query<MemberListQuery>,
) -> ApiResult<Json<Vec<PublicUser>>> {
    admin.ensure_org_scope(&query.org_id)?;
    let users = db::users_by_org(&state.db, &query.org_id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(users.iter().map(PublicUser::from).collect()))
}

/// Deletes a member, revoking their pool keys first.
pub async fn delete_member(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let user = db::user_by_id(&state.db, &id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    // Only members are deletable here; admins are managed by the host (M3+).
    if user.role != "member" {
        return Err(ControlError::Forbidden);
    }
    match &user.org_id {
        Some(org) => admin.ensure_org_scope(org)?,
        None => {
            if admin.role != "host_admin" {
                return Err(ControlError::NotFound);
            }
        }
    }
    db::delete_member(&state.db, &id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Pools
// ---------------------------------------------------------------------------

/// POST /api/v1/pools body.
#[derive(Deserialize)]
pub struct CreatePoolRequest {
    pub org_id: String,
    pub name: String,
    /// Omitted → ControlState.default_models (env config, SPEC §15).
    pub model_allowlist: Option<Vec<String>>,
}

/// GET /api/v1/pools?org_id=… filter (optional).
#[derive(Deserialize)]
pub struct PoolListQuery {
    pub org_id: Option<String>,
}

/// The pool view served by the API (allowlist parsed out of its JSON column).
#[derive(Serialize)]
pub struct PublicPool {
    pub id: String,
    pub org_id: String,
    pub name: String,
    pub kind: String,
    pub model_allowlist: Vec<String>,
    pub status: String,
    pub currency: String,
    pub created_at: i64,
}

/// Parses a pool row's JSON allowlist column into a Vec (empty = unrestricted).
fn parse_allowlist(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

/// Creates a pool inside an org.
pub async fn create_pool(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Json(body): Json<CreatePoolRequest>,
) -> ApiResult<(StatusCode, Json<PublicPool>)> {
    admin.ensure_org_scope(&body.org_id)?;
    let name = body.name.trim();
    if name.is_empty() || name.len() > 200 {
        return Err(ControlError::BadRequest(
            "name must be 1-200 characters".into(),
        ));
    }
    let mut allowlist = body
        .model_allowlist
        .unwrap_or_else(|| state.default_models.clone());
    allowlist.retain(|m| !m.trim().is_empty());
    if allowlist.is_empty() {
        return Err(ControlError::BadRequest(
            "model_allowlist must name at least one model".into(),
        ));
    }
    let pool = db::PoolRow {
        id: new_id(),
        org_id: body.org_id,
        name: name.to_string(),
        kind: "org".to_string(),
        model_allowlist: serde_json::to_string(&allowlist)
            .map_err(|e| ControlError::Internal(e.into()))?,
        status: "active".to_string(),
        currency: "usd".to_string(),
        created_at: now_ms(),
    };
    db::insert_pool(&state.db, &pool)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok((
        StatusCode::CREATED,
        Json(PublicPool {
            id: pool.id,
            org_id: pool.org_id,
            name: pool.name,
            kind: pool.kind,
            model_allowlist: allowlist,
            status: pool.status,
            currency: pool.currency,
            created_at: pool.created_at,
        }),
    ))
}

/// Lists pools: org_admin sees their org's; host_admin sees all (optionally
/// filtered by ?org_id=).
pub async fn list_pools(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Query(query): Query<PoolListQuery>,
) -> ApiResult<Json<Vec<PublicPool>>> {
    let scope = if admin.role == "host_admin" {
        query.org_id.as_deref().map(str::to_string)
    } else {
        admin.org_id.clone()
    };
    let pools = db::list_pools(&state.db, scope.as_deref())
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(
        pools
            .iter()
            .map(|p| PublicPool {
                id: p.id.clone(),
                org_id: p.org_id.clone(),
                name: p.name.clone(),
                kind: p.kind.clone(),
                model_allowlist: parse_allowlist(&p.model_allowlist),
                status: p.status.clone(),
                currency: p.currency.clone(),
                created_at: p.created_at,
            })
            .collect(),
    ))
}

/// Fetches one pool (scoped).
pub async fn get_pool(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path(id): Path<String>,
) -> ApiResult<Json<PublicPool>> {
    let pool = load_scoped_pool(&state, &admin, &id).await?;
    Ok(Json(PublicPool {
        id: pool.id,
        org_id: pool.org_id,
        name: pool.name,
        kind: pool.kind,
        model_allowlist: parse_allowlist(&pool.model_allowlist),
        status: pool.status,
        currency: pool.currency,
        created_at: pool.created_at,
    }))
}

/// Disables a pool (soft delete — usage history stays).
pub async fn disable_pool(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let pool = load_scoped_pool(&state, &admin, &id).await?;
    db::disable_pool(&state.db, &pool.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Loads a pool and enforces the caller's org scope on it.
async fn load_scoped_pool(
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

// ---------------------------------------------------------------------------
// Pool keys (issuing member keys for a pool)
// ---------------------------------------------------------------------------

/// POST /api/v1/pools/{id}/keys body.
#[derive(Deserialize)]
pub struct IssueKeyRequest {
    pub user_id: String,
}

/// Response: the raw pool key, revealed exactly once.
#[derive(Serialize)]
pub struct IssuedKeyResponse {
    pub id: String,
    pub pool_id: String,
    pub user_id: String,
    /// `sk-pool-…` raw key — shown only in THIS response.
    pub raw: String,
    /// Masked display form safe to store/list later.
    pub key_prefix: String,
}

/// Issues a pool key binding a member to a pool.
pub async fn issue_pool_key(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path(pool_id): Path<String>,
    Json(body): Json<IssueKeyRequest>,
) -> ApiResult<(StatusCode, Json<IssuedKeyResponse>)> {
    let pool = load_scoped_pool(&state, &admin, &pool_id).await?;
    let member = db::user_by_id(&state.db, &body.user_id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    // The member must live in the pool's org — a key is org-scoped access.
    if member.org_id.as_deref() != Some(pool.org_id.as_str()) {
        return Err(ControlError::BadRequest(
            "user does not belong to the pool's org".into(),
        ));
    }
    let generated = generate_key();
    let key_id = new_id();
    db::insert_issued_pool_key(
        &state.db,
        &db::NewPoolKey {
            id: key_id.clone(),
            pool_id: pool.id.clone(),
            key_hash: generated.key_hash,
            key_prefix: generated.key_prefix.clone(),
            user_id: member.id.clone(),
            created_at: now_ms(),
        },
    )
    .await
    .map_err(|e| ControlError::Internal(e.into()))?;
    Ok((
        StatusCode::CREATED,
        Json(IssuedKeyResponse {
            id: key_id,
            pool_id: pool.id,
            user_id: member.id,
            raw: generated.raw,
            key_prefix: generated.key_prefix,
        }),
    ))
}

/// Lists a pool's keys (masked).
pub async fn list_pool_keys(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path(pool_id): Path<String>,
) -> ApiResult<Json<Vec<PublicPoolKey>>> {
    let pool = load_scoped_pool(&state, &admin, &pool_id).await?;
    let keys = db::pool_keys_by_pool(&state.db, &pool.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(keys))
}

/// Revokes a pool key.
pub async fn revoke_pool_key(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path((pool_id, key_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let pool = load_scoped_pool(&state, &admin, &pool_id).await?;
    let keys = db::pool_keys_by_pool(&state.db, &pool.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    if !keys.iter().any(|k| k.id == key_id) {
        return Err(ControlError::NotFound);
    }
    db::revoke_pool_key(&state.db, &key_id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Provider keys
// ---------------------------------------------------------------------------

/// POST /api/v1/pools/{id}/provider-keys body. `key` is plaintext ONCE here.
#[derive(Deserialize)]
pub struct CreateProviderKeyRequest {
    pub key: String,
    pub provider: Option<String>,
    pub base_url: Option<String>,
}

/// Masks a raw key into the display form (first-5…last-4, secrets rule).
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

/// Health-light probe result mapped from an upstream /models response.
fn health_from_status(status: Option<u16>) -> &'static str {
    match status {
        Some(s) if (200..300).contains(&s) => "ok",
        Some(401 | 403) => "invalid",
        _ => "unknown", // network errors, 5xx, timeouts — not the key's fault
    }
}

/// Probes `GET {base_url}/models` with the decrypted key (5 s budget).
async fn probe(http: &reqwest::Client, base_url: &str, key: &str) -> Option<u16> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let resp = http
        .get(&url)
        .bearer_auth(key)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await;
    resp.ok().map(|r| r.status().as_u16())
}

/// Creates a provider key: seal → insert (health unknown) → validate probe
/// → record health. The plaintext key exists only inside this handler.
pub async fn create_provider_key(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path(pool_id): Path<String>,
    Json(body): Json<CreateProviderKeyRequest>,
) -> ApiResult<(StatusCode, Json<PublicProviderKey>)> {
    let pool = load_scoped_pool(&state, &admin, &pool_id).await?;
    // Move the secret into Zeroizing immediately: parsed-struct Strings are
    // plain heap memory, and we want drop-time zeroization from here on.
    // (Partial move of one field out of an owned struct — the rest stays.)
    let raw_key = Zeroizing::new(body.key);
    if raw_key.len() < 8 {
        return Err(ControlError::BadRequest(
            "key looks too short to be real".into(),
        ));
    }
    let provider = body.provider.unwrap_or_else(|| "openai".to_string());
    let base_url = body
        .base_url
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        return Err(ControlError::BadRequest("base_url must be http(s)".into()));
    }

    // Seal under this row's identity BEFORE the row exists is impossible, so
    // the id is minted first and doubles as the AD binding.
    let id = new_id();
    let sealed: tributary_crypto::SealedEnvelope = seal(
        &state.master,
        raw_key.as_bytes(),
        &row_ad("provider_keys", &id),
    )
    .map_err(ControlError::Internal)?;
    let row = db::ProviderKeyRow {
        id: id.clone(),
        pool_id: pool.id.clone(),
        provider,
        base_url: base_url.clone(),
        ciphertext: sealed.ciphertext,
        dek_wrapped: sealed.dek_wrapped,
        key_prefix: display_prefix(&raw_key),
        health: "unknown".to_string(),
        last_validated_at: None,
        status: "active".to_string(),
        created_at: now_ms(),
    };
    db::insert_provider_key(&state.db, &row)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;

    // Validate-on-save (kickoff "key health LIGHT").
    let status = probe(&state.http, &base_url, &raw_key).await;
    let health = health_from_status(status);
    db::set_provider_key_health(&state.db, &id, health, now_ms())
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let mut public = PublicProviderKey::from(&row);
    public.health = health.to_string();
    public.last_validated_at = Some(now_ms());
    Ok((StatusCode::CREATED, Json(public)))
}

/// Lists a pool's provider keys (masked — no ciphertext leaves the crate).
pub async fn list_provider_keys(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path(pool_id): Path<String>,
) -> ApiResult<Json<Vec<PublicProviderKey>>> {
    let pool = load_scoped_pool(&state, &admin, &pool_id).await?;
    let keys = db::provider_keys_by_pool(&state.db, &pool.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(keys.iter().map(PublicProviderKey::from).collect()))
}

/// Revokes a provider key.
pub async fn revoke_provider_key(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path((pool_id, key_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let pool = load_scoped_pool(&state, &admin, &pool_id).await?;
    let row = db::provider_key_by_id(&state.db, &key_id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    if row.pool_id != pool.id {
        return Err(ControlError::NotFound);
    }
    db::revoke_provider_key(&state.db, &key_id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Manual revalidation: decrypt (per-use, Zeroizing) → probe → record.
pub async fn revalidate_provider_key(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path((pool_id, key_id)): Path<(String, String)>,
) -> ApiResult<Json<PublicProviderKey>> {
    let pool = load_scoped_pool(&state, &admin, &pool_id).await?;
    let row = db::provider_key_by_id(&state.db, &key_id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    if row.pool_id != pool.id {
        return Err(ControlError::NotFound);
    }
    if row.status != "active" {
        return Err(ControlError::Conflict("provider key is revoked".into()));
    }
    let plaintext = decrypt_provider_key(&state.master, &row)?;
    let status = probe(&state.http, &row.base_url, &plaintext).await;
    let health = health_from_status(status);
    db::set_provider_key_health(&state.db, &key_id, health, now_ms())
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let mut public = PublicProviderKey::from(&row);
    public.health = health.to_string();
    public.last_validated_at = Some(now_ms());
    Ok(Json(public))
}

/// Decrypts a provider-key row into a Zeroizing UTF-8 string (per-use only;
/// never cached — AGENTS secrets convention).
pub fn decrypt_provider_key(
    master: &MasterKey,
    row: &db::ProviderKeyRow,
) -> ApiResult<Zeroizing<String>> {
    let sealed = tributary_crypto::SealedEnvelope {
        ciphertext: row.ciphertext.clone(),
        dek_wrapped: row.dek_wrapped.clone(),
    };
    let bytes = tributary_crypto::open(master, &sealed, &row_ad("provider_keys", &row.id))
        .map_err(|e| {
            ControlError::Internal(
                e.context("provider key decryption failed — master key or row mismatch"),
            )
        })?;
    let text = String::from_utf8(bytes.to_vec())
        .map_err(|_| ControlError::Internal(anyhow::anyhow!("provider key is not valid UTF-8")))?;
    Ok(Zeroizing::new(text))
}

// ---------------------------------------------------------------------------
// Quota rules
// ---------------------------------------------------------------------------

/// POST /api/v1/quota-rules body (upsert of the (subject, period) rule).
#[derive(Deserialize)]
pub struct UpsertQuotaRuleRequest {
    pub subject_kind: String,
    pub subject_id: String,
    pub period: String,
    pub limit_tokens: Option<i64>,
    pub limit_requests: Option<i64>,
}

/// Validates and inserts/replaces one quota rule.
pub async fn upsert_quota_rule(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Json(body): Json<UpsertQuotaRuleRequest>,
) -> ApiResult<(StatusCode, Json<db::QuotaRuleRow>)> {
    if !matches!(body.subject_kind.as_str(), "pool_key" | "user" | "pool") {
        return Err(ControlError::BadRequest(
            "subject_kind must be pool_key, user, or pool".into(),
        ));
    }
    if tributary_common::time::Period::parse(&body.period).is_none() {
        return Err(ControlError::BadRequest(
            "period must be minute, hour, day, or month".into(),
        ));
    }
    if body.limit_tokens.is_none() && body.limit_requests.is_none() {
        return Err(ControlError::BadRequest(
            "at least one of limit_tokens / limit_requests is required".into(),
        ));
    }
    if body.limit_tokens.is_some_and(|v| v < 0) || body.limit_requests.is_some_and(|v| v < 0) {
        return Err(ControlError::BadRequest(
            "limits must be non-negative".into(),
        ));
    }
    // Scope: the subject must resolve inside the caller's org (host: all).
    match db::subject_org(&state.db, &body.subject_kind, &body.subject_id).await {
        Ok(Some(org)) => admin.ensure_org_scope(&org)?,
        Ok(None) if admin.role == "host_admin" => {}
        Ok(None) => return Err(ControlError::NotFound),
        Err(e) => return Err(ControlError::Internal(e.into())),
    }
    let rule = db::QuotaRuleRow {
        id: new_id(),
        subject_kind: body.subject_kind,
        subject_id: body.subject_id,
        period: body.period,
        limit_tokens: body.limit_tokens,
        limit_requests: body.limit_requests,
        limit_cost: 0, // cost limits arrive with the D23 price table
        created_at: now_ms(),
    };
    db::upsert_quota_rule(&state.db, &rule)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok((StatusCode::CREATED, Json(rule)))
}

/// Lists quota rules within the caller's scope.
pub async fn list_quota_rules(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
) -> ApiResult<Json<Vec<db::QuotaRuleRow>>> {
    let all = db::list_quota_rules(&state.db)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let mut scoped = Vec::new();
    for rule in all {
        let org = db::subject_org(&state.db, &rule.subject_kind, &rule.subject_id)
            .await
            .map_err(|e| ControlError::Internal(e.into()))?;
        match org {
            Some(org) => {
                if admin.ensure_org_scope(&org).is_ok() {
                    scoped.push(rule);
                }
            }
            // Unresolvable subjects are host-visible only.
            None if admin.role == "host_admin" => scoped.push(rule),
            None => {}
        }
    }
    Ok(Json(scoped))
}

/// Deletes one quota rule (scoped).
pub async fn delete_quota_rule(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let rule = db::quota_rule_by_id(&state.db, &id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    match db::subject_org(&state.db, &rule.subject_kind, &rule.subject_id).await {
        Ok(Some(org)) => admin.ensure_org_scope(&org)?,
        Ok(None) if admin.role == "host_admin" => {}
        Ok(None) => return Err(ControlError::NotFound),
        Err(e) => return Err(ControlError::Internal(e.into())),
    }
    db::delete_quota_rule(&state.db, &id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(StatusCode::NO_CONTENT)
}
