//! A-026 (M5): D33 class-based enforcement — the sensitive surface grows
//! beyond key lifecycle to quota rules on admin keys, allowlists on admin
//! keys, admin-key MINTING, pool-disabling rules, provider-key revocation
//! (ALWAYS four-eyes at ≥2 admins — director ruling 2026-10-24, NO
//! containment exemption) and org deletion.
//!
//! Every case walks the SAME contract (public API only, workflow step 12):
//! direct endpoint → 403 `approval_required` in a dual-admin org;
//! `POST /orgs/{id}/pending-actions {kind, params}` → 201;
//! distinct admin approves → executed, and the WORLD reflects it
//! (end-state asserted — never just the 403).
//!
//! Ran RED against pre-A-026 code (direct endpoints executed ungated;
//! the request endpoint answered 400 unknown kind) — evidence in
//! target/red-proof/.

mod harness;

use axum::http::StatusCode;
use serde_json::{Value, json};

use harness::*;

/// Dual-admin org with a live stack: pool + provider key + a member key
/// OWNED BY admin B (the sensitive target). Returns
/// (org_id, token_a, token_b, pool_id, b_key_id).
async fn dual_org_stack(
    app: &axum::Router,
    token: &str,
) -> (String, String, String, String, String) {
    let (_, body) = send(
        app,
        post_json(
            "/api/v1/orgs",
            Some(token),
            &json!({"name": "Gates Org", "slug": unique_slug("a026")}),
        ),
    )
    .await;
    let org_id = body["id"].as_str().expect("org").to_string();

    // TWO org admins (the gate counts org_admin users in the org).
    let mut tokens = Vec::new();
    for n in 0..2 {
        let email = unique_email("a026-admin");
        let (status, body) = send(
            app,
            post_json(
                "/api/v1/members",
                Some(token),
                &json!({"org_id": org_id, "email": email, "password": "hunter2hunter2",
                        "role": "org_admin"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "admin #{n}: {body}");
        let (_, body) = send(
            app,
            post_json(
                "/api/v1/auth/login",
                None,
                &json!({"email": email, "password": "hunter2hunter2"}),
            ),
        )
        .await;
        tokens.push(body["token"].as_str().expect("login").to_string());
    }
    let token_a = tokens[0].clone();
    let token_b = tokens[1].clone();

    // Pool + provider key, provisioned by A.
    let (_, pool) = send(
        app,
        post_json(
            "/api/v1/pools",
            Some(&token_a),
            &json!({"org_id": org_id, "name": "gates-pool", "model_allowlist": ["m-test"]}),
        ),
    )
    .await;
    let pool_id = pool["id"].as_str().expect("pool").to_string();
    let (status, _) = send(
        app,
        post_json(
            &format!("/api/v1/pools/{pool_id}/provider-keys"),
            Some(&token_a),
            &json!({"key": "sk-upstream-a026", "base_url": "http://127.0.0.1:9"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "provider key");

    // B's member key in A's pool (B = admin ⇒ the key is admin-owned).
    // MINTING an admin's own first key stays routine here (the A-026
    // mint gate applies to the minting ADMIN's keys; B has none yet and
    // the pool creator A is acting — solo-eligible until B's key exists
    // is NOT the contract; instead B mints their OWN key self-service,
    // which is never sensitive).
    let b_user = tokens_b_user(app, &token_b, &org_id).await;
    let (_, bkey) = send(
        app,
        post_json(
            &format!("/api/v1/pools/{pool_id}/keys"),
            Some(&token_b),
            &json!({"user_id": b_user}),
        ),
    )
    .await;
    let b_key_id = bkey["id"].as_str().expect("B key").to_string();
    (org_id, token_a, token_b, pool_id, b_key_id)
}

/// Resolves the org_admin's user id via the public members API.
async fn tokens_b_user(app: &axum::Router, token_b: &str, org_id: &str) -> String {
    // whoami is the cheapest public identity endpoint.
    let (_, me) = send(app, get_json("/api/v1/auth/me", Some(token_b))).await;
    let id = me["id"]
        .as_str()
        .or(me["user"]["id"].as_str())
        .expect("whoami id");
    let _ = org_id;
    id.to_string()
}

/// Files a pending action and approves it with the OTHER admin.
/// Returns the executed action row.
async fn request_and_approve(
    app: &axum::Router,
    org_id: &str,
    token_a: &str,
    token_b: &str,
    kind: &str,
    params: &Value,
) -> Value {
    let (status, pending) = send(
        app,
        post_json(
            &format!("/api/v1/orgs/{org_id}/pending-actions"),
            Some(token_a),
            &json!({"kind": kind, "params": params}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{kind} request: {pending}");
    let id = pending["id"].as_str().expect("pending id").to_string();
    let (status, decided) = send(
        app,
        post_json(
            &format!("/api/v1/orgs/{org_id}/pending-actions/{id}/approve"),
            Some(token_b),
            &json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{kind} approve: {decided}");
    decided["action"].clone()
}

/// Quota rules on an ADMIN-OWNED key are sensitive; deleting one is
/// sensitive ONLY when the rule has a zero limit (the containment rule —
/// removing a plain cap is routine).
#[tokio::test]
async fn a026_admin_key_quota_and_its_zero_limit_delete() {
    for db in test_dbs().await {
        let (app, _) = control_app(db.pool.clone()).await;
        let token = bootstrap_and_login(&db.pool, &app).await;
        let (org_id, token_a, token_b, _pool_id, b_key_id) = dual_org_stack(&app, &token).await;

        // Direct POST on B's key: refuses in a dual-admin org.
        let (status, body) = send(
            &app,
            post_json(
                "/api/v1/quota-rules",
                Some(&token_a),
                &json!({"subject_kind": "pool_key", "subject_id": b_key_id,
                        "period": "day", "limit_tokens": 500}),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{}: quota on admin key must gate: {body}",
            db.name
        );
        assert_eq!(body["error"]["code"], "approval_required");

        // Queue → approve → the rule EXISTS (end state).
        request_and_approve(
            &app,
            &org_id,
            &token_a,
            &token_b,
            "admin_key_quota",
            &json!({"subject_kind": "pool_key", "subject_id": b_key_id,
                    "period": "day", "limit_tokens": 500}),
        )
        .await;
        let (_, rules) = send(&app, get_json("/api/v1/quota-rules", Some(&token_a))).await;
        let rule500 = rules
            .as_array()
            .expect("rules array")
            .iter()
            .find(|r| r["subject_id"] == json!(b_key_id) && r["limit_tokens"] == json!(500))
            .expect("rule landed after approval")
            .clone();

        // A NON-zero rule may be deleted routinely (boundary pin).
        let rule_id = rule500["id"].as_str().unwrap().to_string();
        let (status, _) = send(
            &app,
            delete_json(&format!("/api/v1/quota-rules/{rule_id}"), Some(&token_a)),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NO_CONTENT,
            "{}: non-zero-limit rule delete is routine",
            db.name
        );

        // A ZERO-limit rule on B's key (full containment): its delete
        // gates too — and executes through the queue.
        let (_, pending) = send(
            &app,
            post_json(
                &format!("/api/v1/orgs/{org_id}/pending-actions"),
                Some(&token_a),
                &json!({"kind": "admin_key_quota",
                        "params": {"subject_kind": "pool_key", "subject_id": b_key_id,
                                   "period": "day", "limit_tokens": 0}}),
            ),
        )
        .await;
        let id0 = pending["id"]
            .as_str()
            .expect("zero rule pending")
            .to_string();
        let (status, _) = send(
            &app,
            post_json(
                &format!("/api/v1/orgs/{org_id}/pending-actions/{id0}/approve"),
                Some(&token_b),
                &json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "zero-limit rule approved");
        let (_, rules0) = send(&app, get_json("/api/v1/quota-rules", Some(&token_a))).await;
        let rule0 = rules0
            .as_array()
            .expect("rules array")
            .iter()
            .find(|r| r["subject_id"] == json!(b_key_id) && r["limit_tokens"] == json!(0))
            .expect("zero-limit rule landed")
            .clone();
        let rule0_id = rule0["id"].as_str().unwrap().to_string();

        // Direct delete of the containment rule: refuses.
        let (status, body) = send(
            &app,
            delete_json(&format!("/api/v1/quota-rules/{rule0_id}"), Some(&token_a)),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{}: containment-rule delete must gate: {body}",
            db.name
        );
        request_and_approve(
            &app,
            &org_id,
            &token_a,
            &token_b,
            "admin_key_quota_delete",
            &json!({"rule_id": rule0_id}),
        )
        .await;
        let (_, after) = send(&app, get_json("/api/v1/quota-rules", Some(&token_a))).await;
        assert!(
            !after
                .as_array()
                .expect("rules array")
                .iter()
                .any(|r| r["id"] == json!(rule0_id)),
            "{}: containment rule gone after approval: {after}",
            db.name
        );
    }
}

/// Allowlist edits on an ADMIN-OWNED key gate (an attacker-admin must not
/// pre-open their own door), add AND remove, through the queue.
#[tokio::test]
async fn a026_allowlist_edits_on_admin_key_gate() {
    for db in test_dbs().await {
        let (app, _) = control_app(db.pool.clone()).await;
        let token = bootstrap_and_login(&db.pool, &app).await;
        let (org_id, token_a, token_b, pool_id, b_key_id) = dual_org_stack(&app, &token).await;

        // Direct add: refuses.
        let (status, body) = send(
            &app,
            post_json(
                &format!("/api/v1/pools/{pool_id}/keys/{b_key_id}/allowlists"),
                Some(&token_a),
                &json!({"cidr": "10.0.0.0/8"}),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{}: allowlist add must gate: {body}",
            db.name
        );

        request_and_approve(
            &app,
            &org_id,
            &token_a,
            &token_b,
            "admin_key_allowlist_add",
            &json!({"key_id": b_key_id, "cidr": "10.0.0.0/8"}),
        )
        .await;
        let (_, entries) = send(
            &app,
            get_json(
                &format!("/api/v1/pools/{pool_id}/keys/{b_key_id}/allowlists"),
                Some(&token_a),
            ),
        )
        .await;
        assert!(
            entries.to_string().contains("10.0.0.0/8"),
            "{}: allowlist landed: {entries}",
            db.name
        );
        let entry_id = entries
            .as_array()
            .and_then(|a| a.first())
            .and_then(|e| e["id"].as_str())
            .expect("entry id")
            .to_string();

        // Direct remove: refuses too; queue → approve → empty.
        let (status, body) = send(
            &app,
            delete_json(
                &format!("/api/v1/pools/{pool_id}/keys/{b_key_id}/allowlists/{entry_id}"),
                Some(&token_a),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{}: allowlist remove must gate: {body}",
            db.name
        );
        request_and_approve(
            &app,
            &org_id,
            &token_a,
            &token_b,
            "admin_key_allowlist_remove",
            &json!({"key_id": b_key_id, "entry_id": entry_id}),
        )
        .await;
        let (_, after) = send(
            &app,
            get_json(
                &format!("/api/v1/pools/{pool_id}/keys/{b_key_id}/allowlists"),
                Some(&token_a),
            ),
        )
        .await;
        assert_eq!(
            after.as_array().map(Vec::len),
            Some(0),
            "{}: allowlist removed: {after}",
            db.name
        );
    }
}

/// MINTING a key for an admin gates; approval mints and reveals ONCE to
/// the approver (reveal-once contract, like rotation).
#[tokio::test]
async fn a026_admin_key_mint_gates_and_reveals_once() {
    for db in test_dbs().await {
        let (app, _) = control_app(db.pool.clone()).await;
        let token = bootstrap_and_login(&db.pool, &app).await;
        let (org_id, token_a, token_b, pool_id, _b_key_id) = dual_org_stack(&app, &token).await;
        let b_user = tokens_b_user(&app, &token_b, &org_id).await;

        // Direct mint for B (an admin): refuses.
        let (status, body) = send(
            &app,
            post_json(
                &format!("/api/v1/pools/{pool_id}/keys"),
                Some(&token_a),
                &json!({"user_id": b_user}),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{}: admin-key mint must gate: {body}",
            db.name
        );

        // Queue → approve: the response reveals the raw ONCE.
        let (status, pending) = send(
            &app,
            post_json(
                &format!("/api/v1/orgs/{org_id}/pending-actions"),
                Some(&token_a),
                &json!({"kind": "admin_key_mint",
                        "params": {"pool_id": pool_id, "user_id": b_user}}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "mint requested: {pending}");
        let id = pending["id"].as_str().unwrap().to_string();
        let (status, decided) = send(
            &app,
            post_json(
                &format!("/api/v1/orgs/{org_id}/pending-actions/{id}/approve"),
                Some(&token_b),
                &json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "mint approve: {decided}");
        let raw = decided["issued"]["raw"].as_str().expect("reveal-once raw");
        let prefix = decided["issued"]["key_prefix"]
            .as_str()
            .expect("reveal carries the masked prefix")
            .to_string();
        assert!(!raw.is_empty(), "reveal carries material");
        assert!(
            !decided["action"].to_string().contains(raw),
            "{}: the action row never echoes the raw",
            db.name
        );

        // End state: the manage list carries the minted row.
        let (_, keys) = send(
            &app,
            get_json(
                &format!("/api/v1/pools/{pool_id}/keys/manage"),
                Some(&token_a),
            ),
        )
        .await;
        assert!(
            keys.as_array()
                .expect("manage array")
                .iter()
                .any(|k| k["key_prefix"] == json!(prefix)),
            "{}: minted key row present: {keys}",
            db.name
        );
    }
}

/// A zero-limit rule ON THE POOL disables the pool — that gates. Deleting
/// such a rule (re-enabling) gates symmetrically.
#[tokio::test]
async fn a026_pool_quota_disable_and_rule_delete_gate() {
    for db in test_dbs().await {
        let (app, _) = control_app(db.pool.clone()).await;
        let token = bootstrap_and_login(&db.pool, &app).await;
        let (org_id, token_a, token_b, pool_id, _b_key_id) = dual_org_stack(&app, &token).await;

        // Direct disable-by-quota: refuses.
        let (status, body) = send(
            &app,
            post_json(
                "/api/v1/quota-rules",
                Some(&token_a),
                &json!({"subject_kind": "pool", "subject_id": pool_id,
                        "period": "day", "limit_tokens": 0}),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{}: pool disable must gate: {body}",
            db.name
        );

        request_and_approve(
            &app,
            &org_id,
            &token_a,
            &token_b,
            "pool_quota_disable",
            &json!({"subject_kind": "pool", "subject_id": pool_id,
                    "period": "day", "limit_tokens": 0}),
        )
        .await;
        let (_, rules) = send(&app, get_json("/api/v1/quota-rules", Some(&token_a))).await;
        let rule = rules
            .as_array()
            .expect("rules array")
            .iter()
            .find(|r| r["subject_id"] == json!(pool_id) && r["limit_tokens"] == json!(0))
            .and_then(|r| r["id"].as_str())
            .expect("pool rule landed")
            .to_string();

        // Direct delete of the disabling rule: refuses; queue → gone.
        let (status, body) = send(
            &app,
            delete_json(&format!("/api/v1/quota-rules/{rule}"), Some(&token_a)),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{}: disabling-rule delete must gate: {body}",
            db.name
        );
        request_and_approve(
            &app,
            &org_id,
            &token_a,
            &token_b,
            "pool_quota_rule_delete",
            &json!({"rule_id": rule}),
        )
        .await;
        let (_, after) = send(&app, get_json("/api/v1/quota-rules", Some(&token_a))).await;
        assert!(
            !after
                .as_array()
                .expect("rules array")
                .iter()
                .any(|r| r["id"] == json!(rule)),
            "{}: pool rule removed: {after}",
            db.name
        );
    }
}

/// Provider-key revocation is 4-eyes ALWAYS at ≥2 admins (director
/// ruling 2026-10-24 — no containment exemption). Solo orgs auto-pass
/// WITH the audit marker.
#[tokio::test]
async fn a026_provider_key_revoke_always_four_eyes() {
    for db in test_dbs().await {
        let (app, _) = control_app(db.pool.clone()).await;
        let token = bootstrap_and_login(&db.pool, &app).await;
        let (org_id, token_a, token_b, pool_id, _b_key_id) = dual_org_stack(&app, &token).await;

        let (_, pks) = send(
            &app,
            get_json(
                &format!("/api/v1/pools/{pool_id}/provider-keys"),
                Some(&token_a),
            ),
        )
        .await;
        let pk_id = pks
            .as_array()
            .and_then(|a| a.first())
            .and_then(|k| k["id"].as_str())
            .expect("provider key id")
            .to_string();

        // Direct revoke: refuses — no flag pretext required or accepted.
        let (status, body) = send(
            &app,
            delete_json(
                &format!("/api/v1/pools/{pool_id}/provider-keys/{pk_id}"),
                Some(&token_a),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{}: provider revoke must gate: {body}",
            db.name
        );

        request_and_approve(
            &app,
            &org_id,
            &token_a,
            &token_b,
            "provider_key_revoke",
            &json!({"key_id": pk_id, "pool_id": pool_id}),
        )
        .await;
        let (_, after) = send(
            &app,
            get_json(
                &format!("/api/v1/pools/{pool_id}/provider-keys"),
                Some(&token_a),
            ),
        )
        .await;
        assert!(
            after
                .as_array()
                .expect("provider keys array")
                .iter()
                .any(|k| k["id"] == json!(pk_id) && k["status"] == "revoked"),
            "{}: provider key revoked (rows persist, status flips): {after}",
            db.name
        );

        // SOLO org: the single admin revokes directly — auto-pass.
        let (_, solo) = send(
            &app,
            post_json(
                "/api/v1/orgs",
                Some(&token),
                &json!({"name": "Solo Org", "slug": unique_slug("solo")}),
            ),
        )
        .await;
        // SOLO = the HOST operates it directly; no org_admin member is
        // provisioned (one would make the org dual-admin by the gate's
        // count).
        let solo_org = solo["id"].as_str().expect("solo org").to_string();
        let (_, pool) = send(
            &app,
            post_json(
                "/api/v1/pools",
                Some(&token),
                &json!({"org_id": solo_org, "name": "solo-pool", "model_allowlist": ["m-test"]}),
            ),
        )
        .await;
        let solo_pool = pool["id"].as_str().expect("solo pool").to_string();
        send(
            &app,
            post_json(
                &format!("/api/v1/pools/{solo_pool}/provider-keys"),
                Some(&token),
                &json!({"key": "sk-solo-a026", "base_url": "http://127.0.0.1:9"}),
            ),
        )
        .await;
        let (_, pks) = send(
            &app,
            get_json(
                &format!("/api/v1/pools/{solo_pool}/provider-keys"),
                Some(&token),
            ),
        )
        .await;
        let solo_pk = pks
            .as_array()
            .and_then(|a| a.first())
            .and_then(|k| k["id"].as_str())
            .expect("solo provider key")
            .to_string();
        let (status, _) = send(
            &app,
            delete_json(
                &format!("/api/v1/pools/{solo_pool}/provider-keys/{solo_pk}"),
                Some(&token),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NO_CONTENT,
            "{}: solo org revokes directly (auto-pass)",
            db.name
        );
    }
}

/// Org deletion gates in dual-admin orgs; approval removes the org.
#[tokio::test]
async fn a026_org_delete_gates() {
    for db in test_dbs().await {
        let (app, _) = control_app(db.pool.clone()).await;
        let token = bootstrap_and_login(&db.pool, &app).await;
        // A POOL-FREE dual-admin org: the delete contract refuses orgs
        // with pools (409) — the gate contract needs a deletable org.
        let (_, body) = send(
            &app,
            post_json(
                "/api/v1/orgs",
                Some(&token),
                &json!({"name": "Bare Org", "slug": unique_slug("bare")}),
            ),
        )
        .await;
        let org_id = body["id"].as_str().expect("bare org").to_string();
        let mut tokens = Vec::new();
        for _ in 0..2 {
            let email = unique_email("bare-admin");
            send(
                &app,
                post_json(
                    "/api/v1/members",
                    Some(&token),
                    &json!({"org_id": org_id, "email": email, "password": "hunter2hunter2",
                            "role": "org_admin"}),
                ),
            )
            .await;
            let (_, body) = send(
                &app,
                post_json(
                    "/api/v1/auth/login",
                    None,
                    &json!({"email": email, "password": "hunter2hunter2"}),
                ),
            )
            .await;
            tokens.push(body["token"].as_str().expect("login").to_string());
        }
        let token_a = tokens[0].clone();
        let token_b = tokens[1].clone();

        let (status, body) = send(
            &app,
            delete_json(&format!("/api/v1/orgs/{org_id}"), Some(&token_a)),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{}: org delete must gate: {body}",
            db.name
        );

        request_and_approve(&app, &org_id, &token_a, &token_b, "org_delete", &json!({})).await;
        let (status, _) = send(
            &app,
            get_json(&format!("/api/v1/orgs/{org_id}"), Some(&token)),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{}: org gone after approved delete",
            db.name
        );
    }
}
