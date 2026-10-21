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
use axum::response::AppendHeaders;
use serde::Deserialize;
use serde::Serialize;
use zeroize::Zeroizing;

use crate::ControlState;
use crate::audit::{self, AuditEntry, TargetKind};
use crate::auth::{self, AuthedAdmin, AuthedUser, client_ip};
use crate::classifier::ClassifierProvider;
use crate::db::{self, PublicProviderKey, PublicUser};
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

/// POST /api/v1/auth/login response — the raw session token (also set as a
/// secure HttpOnly cookie for browser callers), shown once.
#[derive(Serialize)] // serde: this struct → JSON, for response rendering
pub struct LoginResponse {
    pub token: String,
    pub expires_at: i64,
    /// Who just logged in — the dashboard routes by role without a second
    /// round-trip.
    pub user: PublicUser,
}

/// Verifies credentials and mints a session token.
///
/// Order matters (A-004): the throttle gate runs BEFORE any argon2 work —
/// a flooded login endpoint must not double as a CPU-denial-of-service
/// vector. Cookie-authenticated callers get the same session additionally
/// set as a cookie; CLI/bearer callers simply ignore it.
pub async fn login(
    State(state): State<ControlState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<LoginRequest>,
) -> ApiResult<(
    StatusCode,
    AppendHeaders<[(axum::http::HeaderName, String); 1]>,
    Json<LoginResponse>,
)> {
    let email = body.email.trim().to_lowercase();
    let ip = client_ip(&headers);
    // Throttle gate (fixed window per email+IP, A-004) — cheap before any
    // password hashing.
    match state
        .login_throttle
        .check(&state.db, &email, &ip, state.login_max_fails, now_ms())
        .await
    {
        Ok(crate::throttle::ThrottleDecision::Deny { retry_after_secs }) => {
            return Err(ControlError::TooManyRequests { retry_after_secs });
        }
        Ok(crate::throttle::ThrottleDecision::Allow) => {}
        Err(e) => {
            return Err(ControlError::Internal(anyhow::anyhow!(
                "login throttle check failed: {e}"
            )));
        }
    }
    // One generic 401 for unknown email AND wrong password (no probing).
    let user = match db::user_by_email(&state.db, &email).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            state
                .login_throttle
                .record_failure(&state.db, &email, &ip, now_ms())
                .await;
            return Err(ControlError::Unauthorized);
        }
        Err(e) => return Err(ControlError::Internal(e.into())),
    };
    if !auth::verify_password(&user.password_hash, &body.password) {
        state
            .login_throttle
            .record_failure(&state.db, &email, &ip, now_ms())
            .await;
        return Err(ControlError::Unauthorized);
    }
    // Success clears the bucket — a legit user's typos never lock them out.
    state.login_throttle.clear(&state.db, &email, &ip).await;

    let session_id = new_id();
    let (raw, hash, expires_at) = auth::generate_session_token();
    db::insert_session(
        &state.db,
        &session_id,
        &user.id,
        &hash,
        now_ms(),
        expires_at,
    )
    .await
    .map_err(|e| ControlError::Internal(e.into()))?;
    audit::record(
        &state.db,
        AuditEntry {
            org_id: user.org_id.as_deref(),
            actor_id: &user.id,
            actor_email: &user.email,
            action: "session.opened",
            target_kind: TargetKind::Session,
            target_id: &session_id,
            before: None,
            // Sanitized: ids only — no token, no token hash, ever.
            after: Some(serde_json::json!({ "expires_at": expires_at })),
            ip: Some(&ip),
        },
    )
    .await;
    let max_age_secs = (expires_at - now_ms()) / 1000;
    let cookie = auth::session_cookie(&raw, max_age_secs);
    let public = PublicUser::from(&user);
    Ok((
        StatusCode::OK,
        AppendHeaders([(axum::http::header::SET_COOKIE, cookie)]),
        Json(LoginResponse {
            token: raw,
            expires_at,
            user: public,
        }),
    ))
}

/// POST /api/v1/auth/logout — deletes the caller's session and expires the
/// browser cookie. Open to any authenticated user (members included): a
/// member must be able to end their own session.
pub async fn logout(
    State(state): State<ControlState>,
    headers: axum::http::HeaderMap,
    user: AuthedUser,
) -> ApiResult<(
    StatusCode,
    AppendHeaders<[(axum::http::HeaderName, String); 1]>,
)> {
    // AuthedUser already proved a session is valid; resolve it again by the
    // presented token (bearer OR cookie) to learn WHICH session row to drop.
    if let Some(token) = auth::any_token(&headers) {
        let hash = tributary_common::keys::hash_token(&token);
        if let Ok(Some(row)) = db::session_user_by_token_hash(&state.db, &hash).await {
            audit::record(
                &state.db,
                AuditEntry {
                    org_id: user.org_id.as_deref(),
                    actor_id: &user.user_id,
                    actor_email: &user.email,
                    action: "session.closed",
                    target_kind: TargetKind::Session,
                    target_id: &row.session_id,
                    before: None,
                    after: None,
                    ip: Some(&client_ip(&headers)),
                },
            )
            .await;
            let _ = db::delete_session(&state.db, &row.session_id).await;
        }
    }
    Ok((
        StatusCode::NO_CONTENT,
        AppendHeaders([(
            axum::http::header::SET_COOKIE,
            auth::expired_session_cookie(),
        )]),
    ))
}

/// GET /api/v1/auth/me — who is the caller? (Dashboard shell bootstrap.)
pub async fn whoami(user: AuthedUser) -> Json<PublicUser> {
    Json(PublicUser {
        id: user.user_id,
        email: user.email,
        role: user.role,
        org_id: user.org_id,
        created_at: 0, // identity only; the account's age is not needed here
    })
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
    headers: axum::http::HeaderMap,
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
    let public = PublicOrg {
        id: org.id.clone(),
        name: org.name.clone(),
        slug: org.slug.clone(),
        created_at: org.created_at,
    };
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            // The trail belongs to the new org from its first instant.
            org_id: Some(&org.id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "org.created",
            target_kind: TargetKind::Org,
            target_id: &org.id,
            before: None,
            after: serde_json::to_value(&public).ok(),
            ip: Some(&ip),
        },
    )
    .await;
    Ok((StatusCode::CREATED, Json(public)))
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
    headers: axum::http::HeaderMap,
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
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: Some(&org.id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "org.deleted",
            target_kind: TargetKind::Org,
            target_id: &org.id,
            // Masked snapshot of what was removed (name/slug only).
            before: Some(serde_json::json!({ "name": org.name, "slug": org.slug })),
            after: None,
            ip: Some(&ip),
        },
    )
    .await;
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
    /// Optional role escalation: only `org_admin` is recognized, and only
    /// when the caller is a host_admin (hosts provision each org's first
    /// admin; org_admins themselves can only mint plain members).
    pub role: Option<String>,
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
    headers: axum::http::HeaderMap,
    Json(body): Json<CreateMemberRequest>,
) -> ApiResult<(StatusCode, Json<PublicUser>)> {
    admin.ensure_org_scope(&body.org_id)?;
    auth::validate_credentials(&body.email, &body.password)?;
    // Role resolution: default member; `org_admin` needs a host caller;
    // anything else (including `host_admin`) is rejected — the host role
    // is bootstrap-only by design.
    let role = match body.role.as_deref() {
        None | Some("member") => "member",
        Some("org_admin") => {
            if admin.role != "host_admin" {
                return Err(ControlError::Forbidden);
            }
            "org_admin"
        }
        Some(other) => {
            return Err(ControlError::BadRequest(format!(
                "unknown role '{other}' (member or org_admin)"
            )));
        }
    };
    let member = db::UserRow {
        id: new_id(),
        email: body.email.trim().to_lowercase(),
        password_hash: auth::hash_password(&body.password)?,
        role: role.to_string(),
        org_id: Some(body.org_id.clone()),
        created_at: now_ms(),
    };
    db::insert_user(&state.db, &member)
        .await
        .map_err(|e| ControlError::Conflict(format!("email already exists: {e}")))?;
    let public = PublicUser::from(&member);
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: Some(&body.org_id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "member.created",
            target_kind: TargetKind::Member,
            target_id: &member.id,
            before: None,
            // PublicUser is already the masked view (no password hash).
            after: serde_json::to_value(&public).ok(),
            ip: Some(&ip),
        },
    )
    .await;
    Ok((StatusCode::CREATED, Json(public)))
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
    headers: axum::http::HeaderMap,
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
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: user.org_id.as_deref(),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "member.deleted",
            target_kind: TargetKind::Member,
            target_id: &user.id,
            before: serde_json::to_value(PublicUser::from(&user)).ok(),
            after: None,
            ip: Some(&ip),
        },
    )
    .await;
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
    headers: axum::http::HeaderMap,
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
        org_id: body.org_id.clone(),
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
    let public = PublicPool {
        id: pool.id.clone(),
        org_id: pool.org_id.clone(),
        name: pool.name.clone(),
        kind: pool.kind.clone(),
        model_allowlist: allowlist.clone(),
        status: pool.status.clone(),
        currency: pool.currency.clone(),
        created_at: pool.created_at,
    };
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: Some(&pool.org_id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "pool.created",
            target_kind: TargetKind::Pool,
            target_id: &pool.id,
            before: None,
            after: serde_json::to_value(&public).ok(),
            ip: Some(&ip),
        },
    )
    .await;
    Ok((StatusCode::CREATED, Json(public)))
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
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let pool = load_scoped_pool(&state, &admin, &id).await?;
    db::disable_pool(&state.db, &pool.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: Some(&pool.org_id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "pool.disabled",
            target_kind: TargetKind::Pool,
            target_id: &pool.id,
            before: Some(serde_json::json!({ "status": pool.status, "name": pool.name })),
            after: Some(serde_json::json!({ "status": "disabled" })),
            ip: Some(&ip),
        },
    )
    .await;
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
    /// Optional expiry (epoch ms). M4 key governance — unexpired-by-default
    /// stays the ergonomic norm; rotation is the reissue path.
    pub expires_at: Option<i64>,
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
    headers: axum::http::HeaderMap,
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
            expires_at: body.expires_at,
            created_at: now_ms(),
        },
    )
    .await
    .map_err(|e| ControlError::Internal(e.into()))?;
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: Some(&pool.org_id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "pool_key.issued",
            target_kind: TargetKind::PoolKey,
            target_id: &key_id,
            before: None,
            // Masked display prefix ONLY — the raw key is revealed to the
            // admin exactly once, in the HTTP response, and nowhere else
            // (audit sanitization invariant).
            after: Some(serde_json::json!({
                "pool_id": pool.id,
                "user_id": member.id,
                "key_prefix": generated.key_prefix,
                "status": "active",
                "expires_at": body.expires_at,
            })),
            ip: Some(&ip),
        },
    )
    .await;
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

/// Lists a pool's keys in the SHARED export shape (M4): every row — live
/// — rendered with the same five fields and no ownership
/// marker, ordered by `created_at` (canaries are backdated at plant time so
/// they interleave with issuance history). Ownership
/// are served only by [`manage_pool_keys`]. Director directive, M4
/// kickoff: an insider scraping this payload must not be able to filter
/// out the canaries.
pub async fn list_pool_keys(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path(pool_id): Path<String>,
) -> ApiResult<Json<Vec<db::SharedPoolKey>>> {
    let pool = load_scoped_pool(&state, &admin, &pool_id).await?;
    let keys = db::shared_pool_keys_by_pool(&state.db, &pool.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(keys))
}

/// Revokes a pool key.
pub async fn revoke_pool_key(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    headers: axum::http::HeaderMap,
    Path((pool_id, key_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let pool = load_scoped_pool(&state, &admin, &pool_id).await?;
    let keys = db::pool_keys_by_pool(&state.db, &pool.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let key = keys
        .iter()
        .find(|k| k.id == key_id)
        .ok_or(ControlError::NotFound)?;
    db::revoke_pool_key(&state.db, &key_id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: Some(&pool.org_id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "pool_key.revoked",
            target_kind: TargetKind::PoolKey,
            target_id: &key.id,
            before: Some(serde_json::json!({ "status": key.status, "key_prefix": key.key_prefix })),
            after: Some(serde_json::json!({ "status": "revoked", "key_prefix": key.key_prefix })),
            ip: Some(&ip),
        },
    )
    .await;
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
    headers: axum::http::HeaderMap,
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
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: Some(&pool.org_id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "provider_key.created",
            target_kind: TargetKind::ProviderKey,
            target_id: &id,
            before: None,
            // PublicProviderKey is the masked view — prefix + last-4 only;
            // ciphertext/dek never cross into the audit trail (invariant).
            after: serde_json::to_value(&public).ok(),
            ip: Some(&ip),
        },
    )
    .await;
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
    headers: axum::http::HeaderMap,
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
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: Some(&pool.org_id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "provider_key.revoked",
            target_kind: TargetKind::ProviderKey,
            target_id: &key_id,
            before: Some(serde_json::json!({ "status": row.status, "key_prefix": row.key_prefix })),
            after: Some(serde_json::json!({ "status": "revoked", "key_prefix": row.key_prefix })),
            ip: Some(&ip),
        },
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Manual revalidation: decrypt (per-use, Zeroizing) → probe → record.
pub async fn revalidate_provider_key(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    headers: axum::http::HeaderMap,
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
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: Some(&pool.org_id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "provider_key.revalidated",
            target_kind: TargetKind::ProviderKey,
            target_id: &key_id,
            before: Some(serde_json::json!({ "health": row.health })),
            after: Some(serde_json::json!({ "health": health, "key_prefix": row.key_prefix })),
            ip: Some(&ip),
        },
    )
    .await;
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
    headers: axum::http::HeaderMap,
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
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            // The subject's org owns the trail (host actions included).
            org_id: db::subject_org(&state.db, &rule.subject_kind, &rule.subject_id)
                .await
                .ok()
                .flatten()
                .as_deref(),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "quota_rule.upserted",
            target_kind: TargetKind::QuotaRule,
            target_id: &rule.id,
            before: None,
            after: serde_json::to_value(&rule).ok(),
            ip: Some(&ip),
        },
    )
    .await;
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
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let rule = db::quota_rule_by_id(&state.db, &id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    let org = match db::subject_org(&state.db, &rule.subject_kind, &rule.subject_id).await {
        Ok(Some(org)) => Some(org),
        Ok(None) if admin.role == "host_admin" => None,
        Ok(None) => return Err(ControlError::NotFound),
        Err(e) => return Err(ControlError::Internal(e.into())),
    };
    if let Some(org) = &org {
        admin.ensure_org_scope(org)?;
    }
    db::delete_quota_rule(&state.db, &id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: org.as_deref(),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "quota_rule.deleted",
            target_kind: TargetKind::QuotaRule,
            target_id: &rule.id,
            before: serde_json::to_value(&rule).ok(),
            after: None,
            ip: Some(&ip),
        },
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Audit log (admin viewer)
// ---------------------------------------------------------------------------

/// GET /api/v1/audit?limit=&offset= — the control-plane trail, newest first.
///
/// org_admin sees only rows recorded against their org (the scope decision
/// was made at WRITE time via `org_id`); host_admin sees everything.
#[derive(Deserialize)]
pub struct AuditListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Lists audit rows within the caller's scope.
pub async fn list_audit(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Query(query): Query<AuditListQuery>,
) -> ApiResult<Json<Vec<crate::audit::AuditRow>>> {
    let scope = if admin.role == "host_admin" {
        None
    } else {
        admin.org_id.as_deref()
    };
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0).max(0);
    let rows = crate::audit::list(&state.db, scope, limit, offset)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(rows))
}

// ---------------------------------------------------------------------------
// M4: key governance — management view, rotation, expiry, IP allowlists
// ---------------------------------------------------------------------------

/// Loads a pool AND one of its keys, enforcing the caller's org scope on
/// both (a key from another pool under the same org id is still a 404).
async fn load_scoped_key(
    state: &ControlState,
    admin: &AuthedAdmin,
    pool_id: &str,
    key_id: &str,
) -> ApiResult<(db::PoolRow, db::ManagedPoolKey)> {
    let pool = load_scoped_pool(state, admin, pool_id).await?;
    let keys = db::managed_pool_keys_by_pool(&state.db, &pool.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let key = keys
        .into_iter()
        .find(|k| k.id == key_id)
        .ok_or(ControlError::NotFound)?;
    Ok((pool, key))
}

/// GET /api/v1/pools/{id}/keys/manage — the admin management view: kind,
/// owner, expiry, status. The dashboard consumes this, not the
/// shared export listing.
pub async fn manage_pool_keys(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path(pool_id): Path<String>,
) -> ApiResult<Json<Vec<db::ManagedPoolKey>>> {
    let pool = load_scoped_pool(&state, &admin, &pool_id).await?;
    let keys = db::managed_pool_keys_by_pool(&state.db, &pool.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(keys))
}

/// Response for rotation: the replacement key, revealed exactly once.
#[derive(Serialize)]
pub struct RotatedKeyResponse {
    /// The NEW key's row id (the old row keeps its own id, status rotated).
    pub id: String,
    pub pool_id: String,
    pub user_id: Option<String>,
    pub raw: String,
    pub key_prefix: String,
    /// The replaced row id, for the audit trail/UI lineage.
    pub replaced_key_id: String,
}

/// POST /api/v1/pools/{id}/keys/{key_id}/rotate — key rotation (reissue):
/// mints a fresh key for the same member, marks the old row 'rotated'
/// (distinct from 'revoked' — rotation is hygiene, not incident response),
/// preserves the expiry policy.
pub async fn rotate_pool_key(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    headers: axum::http::HeaderMap,
    Path((pool_id, key_id)): Path<(String, String)>,
) -> ApiResult<(StatusCode, Json<RotatedKeyResponse>)> {
    let (pool, old) = load_scoped_key(&state, &admin, &pool_id, &key_id).await?;
    if old.status != "active" && old.status != "suspended" {
        return Err(ControlError::Conflict(format!(
            "key is {} — only active or suspended keys rotate",
            old.status
        )));
    }
    let generated = generate_key();
    let new_key_id = new_id();
    db::insert_issued_pool_key(
        &state.db,
        &db::NewPoolKey {
            id: new_key_id.clone(),
            pool_id: pool.id.clone(),
            key_hash: generated.key_hash,
            key_prefix: generated.key_prefix.clone(),
            user_id: old
                .user_id
                .clone()
                .ok_or(ControlError::Conflict("key has no owner".into()))?,
            expires_at: old.expires_at,
            created_at: now_ms(),
        },
    )
    .await
    .map_err(|e| ControlError::Internal(e.into()))?;
    db::rotate_pool_key(&state.db, &old.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: Some(&pool.org_id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "pool_key.rotated",
            target_kind: TargetKind::PoolKey,
            target_id: &new_key_id,
            before: Some(serde_json::json!({
                "id": old.id, "status": old.status, "key_prefix": old.key_prefix,
            })),
            after: Some(serde_json::json!({
                "id": new_key_id,
                "replaces": old.id,
                "key_prefix": generated.key_prefix,
                "status": "active",
            })),
            ip: Some(&ip),
        },
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(RotatedKeyResponse {
            id: new_key_id,
            pool_id: pool.id,
            user_id: old.user_id,
            raw: generated.raw,
            key_prefix: generated.key_prefix,
            replaced_key_id: old.id,
        }),
    ))
}

/// POST /api/v1/pools/{id}/keys/{key_id}/expiry body (`null` clears).
#[derive(Deserialize)]
pub struct SetExpiryRequest {
    pub expires_at: Option<i64>,
}

/// POST /api/v1/pools/{id}/keys/{key_id}/expiry — set or clear a key's
/// expiry (epoch ms). The gateway already rejects expired keys at auth.
pub async fn set_key_expiry(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    headers: axum::http::HeaderMap,
    Path((pool_id, key_id)): Path<(String, String)>,
    Json(body): Json<SetExpiryRequest>,
) -> ApiResult<StatusCode> {
    let (pool, key) = load_scoped_key(&state, &admin, &pool_id, &key_id).await?;
    if let Some(at) = body.expires_at
        && at <= now_ms()
    {
        return Err(ControlError::BadRequest(
            "expires_at must be in the future (revoke instead of expiring into the past)".into(),
        ));
    }
    db::set_pool_key_expiry(&state.db, &key.id, body.expires_at)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: Some(&pool.org_id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "pool_key.expiry_set",
            target_kind: TargetKind::PoolKey,
            target_id: &key.id,
            before: Some(serde_json::json!({ "expires_at": key.expires_at })),
            after: Some(serde_json::json!({ "expires_at": body.expires_at })),
            ip: Some(&ip),
        },
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/pools/{id}/keys/{key_id}/allowlists body.
#[derive(Deserialize)]
pub struct AddAllowlistRequest {
    pub cidr: String,
    pub note: Option<String>,
}

/// Adds one CIDR to a key's IP allowlist. The gateway evaluates the whole
/// set per request (fail-closed: no rows = unrestricted — dormant default).
pub async fn add_key_allowlist(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    headers: axum::http::HeaderMap,
    Path((pool_id, key_id)): Path<(String, String)>,
    Json(body): Json<AddAllowlistRequest>,
) -> ApiResult<(StatusCode, Json<db::IpAllowlistRow>)> {
    let (pool, key) = load_scoped_key(&state, &admin, &pool_id, &key_id).await?;
    // Parse-validate BEFORE storing: garbage CIDRs would silently become
    // deny-everything rows at the gateway (fail-closed matching).
    if tributary_common::net::Cidr::parse(&body.cidr).is_none() {
        return Err(ControlError::BadRequest(format!(
            "cidr '{}' is not a valid IPv4/IPv6 network (e.g. 10.0.0.0/8)",
            body.cidr
        )));
    }
    let id = new_id();
    db::insert_ip_allowlist(
        &state.db,
        &id,
        &key.id,
        body.cidr.trim(),
        body.note
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty()),
        now_ms(),
    )
    .await
    .map_err(|e| ControlError::Conflict(format!("cidr already allowlisted: {e}")))?;
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: Some(&pool.org_id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "ip_allowlist.added",
            target_kind: TargetKind::PoolKey,
            target_id: &key.id,
            before: None,
            after: Some(serde_json::json!({
                "cidr": body.cidr.trim(),
                "note": body.note,
                "key_prefix": key.key_prefix,
            })),
            ip: Some(&ip),
        },
    )
    .await;
    let row = db::ip_allowlists_for_key(&state.db, &key.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or(ControlError::Internal(anyhow::anyhow!(
            "allowlist row vanished"
        )))?;
    Ok((StatusCode::CREATED, Json(row)))
}

/// GET /api/v1/pools/{id}/keys/{key_id}/allowlists — the CIDR list for the
/// key-governance editor. Read-only; no audit noise per view.
pub async fn list_key_allowlists(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path((pool_id, key_id)): Path<(String, String)>,
) -> ApiResult<Json<Vec<db::IpAllowlistRow>>> {
    let (_pool, key) = load_scoped_key(&state, &admin, &pool_id, &key_id).await?;
    let rows = db::ip_allowlists_for_key(&state.db, &key.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(rows))
}

/// Removes one CIDR row from a key's allowlist.
pub async fn remove_key_allowlist(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    headers: axum::http::HeaderMap,
    Path((pool_id, key_id, entry_id)): Path<(String, String, String)>,
) -> ApiResult<StatusCode> {
    let (pool, key) = load_scoped_key(&state, &admin, &pool_id, &key_id).await?;
    let entries = db::ip_allowlists_for_key(&state.db, &key.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let entry = entries
        .iter()
        .find(|e| e.id == entry_id)
        .ok_or(ControlError::NotFound)?;
    let removed = entry.clone();
    db::delete_ip_allowlist(&state.db, &entry.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: Some(&pool.org_id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "ip_allowlist.removed",
            target_kind: TargetKind::PoolKey,
            target_id: &key.id,
            before: Some(serde_json::json!({
                "cidr": removed.cidr, "key_prefix": key.key_prefix,
            })),
            after: None,
            ip: Some(&ip),
        },
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// M4: review queue (admin) — the free-tier response surface
// ---------------------------------------------------------------------------

/// GET /api/v1/review-flags query.
#[derive(Deserialize)]
pub struct ReviewFlagListQuery {
    /// 'open' (default) | 'resolved' | 'all'.
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Lists review flags within the caller's scope (org_admin: their org;
/// host_admin: every org).
pub async fn list_review_flags(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Query(query): Query<ReviewFlagListQuery>,
) -> ApiResult<Json<Vec<db::ReviewFlagRow>>> {
    let scope = if admin.role == "host_admin" {
        None
    } else {
        admin.org_id.as_deref()
    };
    let status = match query.status.as_deref() {
        None | Some("open") => Some("open"),
        Some("resolved") => Some("resolved"),
        Some("all") => None,
        Some(other) => {
            return Err(ControlError::BadRequest(format!(
                "status filter must be open, resolved, or all (got '{other}')"
            )));
        }
    };
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0).max(0);
    let rows = db::list_review_flags(&state.db, scope, status, limit, offset)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(rows))
}

/// POST /api/v1/review-flags/{id}/resolve body.
#[derive(Deserialize)]
pub struct ResolveFlagRequest {
    /// 'unblock' (restore a suspended key), 'revoke_key' (kill it), or
    /// 'dismiss' (no key action).
    pub action: String,
}

/// Resolves one review flag — the human decision step (D17: detection is
/// free, the RESPONSE is manual in the free tier). One-click unblock is
/// the auto-block recovery path (D28: never auto-ban).
pub async fn resolve_review_flag(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ResolveFlagRequest>,
) -> ApiResult<StatusCode> {
    let flag = db::review_flag_by_id(&state.db, &id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    admin.ensure_org_scope(&flag.org_id)?;
    if flag.status != "open" {
        return Err(ControlError::Conflict("flag is already resolved".into()));
    }
    let pool = match &flag.pool_id {
        Some(pool_id) => Some(
            db::pool_by_id(&state.db, pool_id)
                .await
                .map_err(|e| ControlError::Internal(e.into()))?
                .ok_or(ControlError::NotFound)?,
        ),
        None => None,
    };
    match body.action.as_str() {
        "unblock" => {
            let key_id = flag.pool_key_id.as_deref().ok_or(ControlError::BadRequest(
                "flag has no key to unblock — use dismiss".into(),
            ))?;
            let n = db::unsuspend_pool_key(&state.db, key_id)
                .await
                .map_err(|e| ControlError::Internal(e.into()))?;
            if n == 0 {
                return Err(ControlError::Conflict(
                    "key is not suspended (already unblocked or revoked?)".into(),
                ));
            }
        }
        "revoke_key" => {
            let key_id = flag.pool_key_id.as_deref().ok_or(ControlError::BadRequest(
                "flag has no key to revoke — use dismiss".into(),
            ))?;
            db::revoke_pool_key(&state.db, key_id)
                .await
                .map_err(|e| ControlError::Internal(e.into()))?;
        }
        "dismiss" => {}
        other => {
            return Err(ControlError::BadRequest(format!(
                "action must be unblock, revoke_key, or dismiss (got '{other}')"
            )));
        }
    }
    db::resolve_review_flag(&state.db, &flag.id, &admin.user_id, now_ms())
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: Some(&flag.org_id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "review_flag.resolved",
            target_kind: TargetKind::ReviewFlag,
            target_id: &flag.id,
            before: Some(serde_json::json!({
                "kind": flag.kind, "severity": flag.severity, "status": flag.status,
            })),
            after: Some(serde_json::json!({ "action": body.action })),
            ip: Some(&ip),
        },
    )
    .await;
    // The key action (if any) gets its own trail row for key-centric views.
    if let (Some(key_id), Some(pool)) = (&flag.pool_key_id, &pool)
        && body.action != "dismiss"
    {
        audit::record(
            &state.db,
            AuditEntry {
                org_id: Some(&pool.org_id),
                actor_id: &admin.user_id,
                actor_email: &admin.email,
                action: if body.action == "unblock" {
                    "pool_key.unblocked"
                } else {
                    "pool_key.revoked"
                },
                target_kind: TargetKind::PoolKey,
                target_id: key_id,
                before: Some(serde_json::json!({ "status": "suspended" })),
                after: Some(serde_json::json!({
                    "status": if body.action == "unblock" { "active" } else { "revoked" },
                    "via_review_flag": flag.id,
                })),
                ip: Some(&ip),
            },
        )
        .await;
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// M4: BYOK purpose classifier config (D28)
// ---------------------------------------------------------------------------

/// PUT /api/v1/orgs/{id}/classifier body. `key` is plaintext ONCE here.
#[derive(Deserialize)]
pub struct PutClassifierRequest {
    pub base_url: String,
    /// Wire contract: "jev" (TypeSafe systemone, default) or "custom"
    /// (generic 0..1 scorer). Unknown values are rejected below.
    pub dialect: Option<String>,
    pub model: String,
    pub key: String,
    pub purpose_statement: String,
    /// 'alert' (default) | 'auto_block'.
    pub mode: Option<String>,
    pub alert_threshold: Option<f64>,
    pub auto_block_threshold: Option<f64>,
    pub sampling_per_hour: Option<i64>,
    /// REQUIRED true when mode = auto_block: the admin has seen the
    /// auto-block warning (shadow-first, suspends keys — never bans).
    pub acknowledge_auto_block: Option<bool>,
}

/// PUT /api/v1/orgs/{id}/settings body (D32 knobs).
#[derive(Deserialize)]
pub struct PutOrgSettingsRequest {
    /// Sliding window for the distinct-IP concurrency detector, seconds.
    /// Clamped to 60–86400: below a minute is alert spam, above a day the
    /// "concurrent" claim stops being honest.
    pub concurrency_window_secs: i64,
}

/// GET /api/v1/orgs/{id}/settings — defaults are returned even when the
/// org never tuned anything (no row exists).
pub async fn get_org_settings(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path(org_id): Path<String>,
) -> ApiResult<Json<db::OrgSettings>> {
    let org = db::org_by_id(&state.db, &org_id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    admin.ensure_org_scope(&org.id)?;
    let settings = db::org_settings(&state.db, &org.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(settings))
}

/// PUT /api/v1/orgs/{id}/settings — tune the D32 window. Takes effect on
/// the next request (the gateway caches settings for 5 s, D9 pattern).
pub async fn put_org_settings(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    headers: axum::http::HeaderMap,
    Path(org_id): Path<String>,
    Json(body): Json<PutOrgSettingsRequest>,
) -> ApiResult<Json<db::OrgSettings>> {
    let org = db::org_by_id(&state.db, &org_id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    admin.ensure_org_scope(&org.id)?;
    if !(60..=86400).contains(&body.concurrency_window_secs) {
        return Err(ControlError::BadRequest(format!(
            "concurrency_window_secs must be within 60–86400 (got {})",
            body.concurrency_window_secs
        )));
    }
    let client_ip = tributary_common::net::client_ip(&headers);
    let updated_at = tributary_common::db::now_ms();
    db::upsert_org_settings(&state.db, &org.id, body.concurrency_window_secs, updated_at)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    audit::record(
        &state.db,
        audit::AuditEntry {
            org_id: Some(&org.id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "org.settings_updated",
            target_kind: audit::TargetKind::Org,
            target_id: &org.id,
            before: None,
            after: Some(serde_json::json!({
                "concurrency_window_secs": body.concurrency_window_secs
            })),
            ip: Some(&client_ip),
        },
    )
    .await;
    let settings = db::org_settings(&state.db, &org.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(settings))
}

/// GET /api/v1/orgs/{id}/classifier — the masked config + shadow stats.
/// 404 when unconfigured (dormant is the absence of a row, D28).
pub async fn get_classifier(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    Path(org_id): Path<String>,
) -> ApiResult<Json<db::PublicClassifierConfig>> {
    let org = db::org_by_id(&state.db, &org_id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    admin.ensure_org_scope(&org.id)?;
    let row = db::classifier_config_by_org(&state.db, &org.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    let public = db::public_classifier_config(&state.db, &row)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(public))
}

/// PUT /api/v1/orgs/{id}/classifier — configure (or reconfigure) the
/// classifier. The key is PROBED against the provider at save time
/// (validates key + model; D28); only a passing probe persists. Entering
/// auto_block mode requires `acknowledge_auto_block` and always starts a
/// fresh SHADOW window (1–2 weeks of "would have blocked N" before an
/// explicit go-live — D28).
pub async fn put_classifier(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    headers: axum::http::HeaderMap,
    Path(org_id): Path<String>,
    Json(body): Json<PutClassifierRequest>,
) -> ApiResult<(StatusCode, Json<db::PublicClassifierConfig>)> {
    let org = db::org_by_id(&state.db, &org_id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    admin.ensure_org_scope(&org.id)?;
    // Move the secret into Zeroizing immediately (secrets convention).
    let raw_key = Zeroizing::new(body.key);
    if raw_key.len() < 8 {
        return Err(ControlError::BadRequest(
            "classifier key looks too short to be real".into(),
        ));
    }
    // Empty base_url defaults to the hosted Jev endpoint (director
    // research: hosted only, no self-host variant) — one less thing for an
    // admin to get wrong. Explicit http(s) URLs override (tests, local
    // mocks).
    let base_url = if body.base_url.trim().is_empty() {
        crate::classifier::JEV_DEFAULT_BASE_URL.to_string()
    } else {
        body.base_url.trim().trim_end_matches('/').to_string()
    };
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        return Err(ControlError::BadRequest("base_url must be http(s)".into()));
    }
    let model = body.model.trim();
    if model.is_empty() || model.len() > 200 {
        return Err(ControlError::BadRequest(
            "model must be 1-200 characters".into(),
        ));
    }
    let purpose = body.purpose_statement.trim();
    if purpose.is_empty() || purpose.len() > 2000 {
        return Err(ControlError::BadRequest(
            "purpose_statement must be 1-2000 characters".into(),
        ));
    }
    let mode = match body.mode.as_deref() {
        None | Some("alert") => "alert",
        Some("auto_block") => {
            if body.acknowledge_auto_block != Some(true) {
                return Err(ControlError::BadRequest(
                    "auto_block requires acknowledge_auto_block=true — it suspends member keys \
                     below the alignment threshold; run the shadow window first (D28)"
                        .into(),
                ));
            }
            "auto_block"
        }
        Some(other) => {
            return Err(ControlError::BadRequest(format!(
                "mode must be alert or auto_block (got '{other}')"
            )));
        }
    };
    let alert_bp = threshold_bp(body.alert_threshold.unwrap_or(0.5))?;
    let block_bp = threshold_bp(body.auto_block_threshold.unwrap_or(0.15))?;
    let sampling = body.sampling_per_hour.unwrap_or(30).clamp(1, 100_000);

    // Dialect: which wire contract the endpoint speaks (director research
    // 2026-10-21). Defaults to the REAL hosted Jev contract; "custom" is
    // the generic shape this crate originally shipped.
    let dialect = match body.dialect.as_deref() {
        None | Some("jev") => "jev",
        Some("custom") => "custom",
        Some(other) => {
            return Err(ControlError::BadRequest(format!(
                "dialect must be 'jev' or 'custom' (got '{other}')"
            )));
        }
    };

    // Save-time probe (D28): the provider must accept this key + model and
    // return a parseable score, or nothing is stored. For the jev dialect
    // this is ONE minimal real noul call (cost ≈ nothing; there is no
    // /models endpoint to lean on) — it validates key + model + shape in
    // one shot.
    let provider = crate::classifier::JevClassifier::new(
        state.http.clone(),
        &base_url,
        model,
        crate::classifier::Dialect::parse(dialect),
        raw_key.clone(),
    );
    let probe = provider
        .score(crate::classifier::ScoreRequest {
            model,
            // Probe with the org's REAL purpose: the saved configuration
            // is what must work, not a placeholder (the instructions
            // embed the purpose; a too-long or malformed statement should
            // fail HERE, at save time, not on the first real request).
            purpose,
            text: "Tributary save-time probe",
        })
        .await;
    if let Err(err) = probe {
        return Err(ControlError::BadRequest(format!(
            "classifier probe failed — nothing was saved: {err:#}"
        )));
    }

    // Seal under this org's row identity (rule 2: envelope encryption, the
    // same construction as provider keys).
    let sealed = seal(
        &state.master,
        raw_key.as_bytes(),
        &row_ad("classifier_config", &org.id),
    )
    .map_err(ControlError::Internal)?;
    let row = db::ClassifierConfigRow {
        org_id: org.id.clone(),
        base_url,
        dialect: dialect.to_string(),
        model: model.to_string(),
        ciphertext: sealed.ciphertext,
        dek_wrapped: sealed.dek_wrapped,
        key_prefix: display_prefix(&raw_key),
        purpose_statement: purpose.to_string(),
        mode: mode.to_string(),
        alert_threshold_bp: alert_bp,
        block_threshold_bp: block_bp,
        // Entering (or re-entering) auto_block ALWAYS starts in shadow —
        // go-live is a separate explicit decision.
        auto_block_live: false,
        shadow_started_at: if mode == "auto_block" {
            Some(now_ms())
        } else {
            None
        },
        sampling_per_hour: sampling,
        updated_at: now_ms(),
    };
    db::upsert_classifier_config(&state.db, &row)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let public = db::public_classifier_config(&state.db, &row)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: Some(&org.id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "classifier.configured",
            target_kind: TargetKind::ClassifierConfig,
            target_id: &org.id,
            before: None,
            // Masked view only — the sealed key material never crosses
            // into the audit trail.
            after: serde_json::to_value(&public).ok(),
            ip: Some(&ip),
        },
    )
    .await;
    Ok((StatusCode::CREATED, Json(public)))
}

/// Validates a 0..=1 float threshold into basis points (exact integer
/// storage — see the migration note on why scores avoid REAL columns).
fn threshold_bp(value: f64) -> ApiResult<i64> {
    if !(0.0..=1.0).contains(&value) {
        return Err(ControlError::BadRequest(
            "thresholds must be between 0.0 and 1.0".into(),
        ));
    }
    Ok((value * 10_000.0).round() as i64)
}

/// DELETE /api/v1/orgs/{id}/classifier — remove the config; the feature
/// goes dormant (no row ⇒ no classification, D28).
pub async fn delete_classifier(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    headers: axum::http::HeaderMap,
    Path(org_id): Path<String>,
) -> ApiResult<StatusCode> {
    let org = db::org_by_id(&state.db, &org_id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    admin.ensure_org_scope(&org.id)?;
    let existing = db::classifier_config_by_org(&state.db, &org.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let Some(existing) = existing else {
        return Err(ControlError::NotFound);
    };
    db::delete_classifier_config(&state.db, &org.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: Some(&org.id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "classifier.deleted",
            target_kind: TargetKind::ClassifierConfig,
            target_id: &org.id,
            before: Some(serde_json::json!({
                "mode": existing.mode,
                "auto_block_live": existing.auto_block_live,
                "key_prefix": existing.key_prefix,
            })),
            after: None,
            ip: Some(&ip),
        },
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/orgs/{id}/classifier/go-live — flip auto-block from shadow
/// to live. The admin should review the shadow stats first (the GET view
/// shows "would have blocked N from M members"); the endpoint itself never
/// auto-triggers (D28: explicit decision, never automatic).
pub async fn classifier_go_live(
    State(state): State<ControlState>,
    admin: AuthedAdmin,
    headers: axum::http::HeaderMap,
    Path(org_id): Path<String>,
) -> ApiResult<Json<db::PublicClassifierConfig>> {
    let org = db::org_by_id(&state.db, &org_id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    admin.ensure_org_scope(&org.id)?;
    let n = db::classifier_go_live(&state.db, &org.id, now_ms())
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    if n == 0 {
        return Err(ControlError::Conflict(
            "no auto_block config in shadow — configure it first (PUT with acknowledge)".into(),
        ));
    }
    let row = db::classifier_config_by_org(&state.db, &org.id)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?
        .ok_or(ControlError::NotFound)?;
    let ip = client_ip(&headers);
    audit::record(
        &state.db,
        AuditEntry {
            org_id: Some(&org.id),
            actor_id: &admin.user_id,
            actor_email: &admin.email,
            action: "classifier.go_live",
            target_kind: TargetKind::ClassifierConfig,
            target_id: &org.id,
            before: Some(serde_json::json!({ "auto_block_live": false })),
            after: Some(serde_json::json!({ "auto_block_live": true })),
            ip: Some(&ip),
        },
    )
    .await;
    let public = db::public_classifier_config(&state.db, &row)
        .await
        .map_err(|e| ControlError::Internal(e.into()))?;
    Ok(Json(public))
}
