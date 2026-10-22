//! Member self-service isolation tests (M3): a member sees and touches ONLY
//! their own keys and usage — never another member's, never org-wide data.
//! The pre-M5 surfacing chore added the expiry/cap payload journey here.

mod harness;

use axum::http::StatusCode;
use serde_json::json;

use harness::*;

/// Provisions org → pool → two members (A, B) → one key each → three usage
/// rows (A×2, B×1). Returns everything the assertions need.
struct Fixture {
    app: axum::Router,
    _host: String,
    pool_id: String,
    token_a: String,
    token_b: String,
    key_a: (String, String), // (id, masked prefix)
    key_b: (String, String),
}

async fn provision(db: &tributary_common::db::Db) -> Fixture {
    let (app, _mock) = control_app(db.clone()).await;
    let host = bootstrap_and_login(db, &app).await;
    let (_, org) = send(
        &app,
        post_json(
            "/api/v1/orgs",
            Some(&host),
            &json!({"name": "Iso", "slug": unique_slug("iso")}),
        ),
    )
    .await;
    let org_id = org["id"].as_str().unwrap().to_string();
    let (_, pool) = send(
        &app,
        post_json(
            "/api/v1/pools",
            Some(&host),
            &json!({"org_id": org_id, "name": "main"}),
        ),
    )
    .await;
    let pool_id = pool["id"].as_str().unwrap().to_string();

    let mut members = Vec::new();
    for n in ["a", "b"] {
        let email = unique_email(n);
        let (_, member) = send(
            &app,
            post_json(
                "/api/v1/members",
                Some(&host),
                &json!({"org_id": org_id, "email": email, "password": "password123"}),
            ),
        )
        .await;
        let (_, login) = send(
            &app,
            post_json(
                "/api/v1/auth/login",
                None,
                &json!({"email": email, "password": "password123"}),
            ),
        )
        .await;
        members.push((
            member["id"].as_str().unwrap().to_string(),
            login["token"].as_str().unwrap().to_string(),
        ));
    }

    let mut keys = Vec::new();
    for (user_id, _) in &members {
        let (_, key) = send(
            &app,
            post_json(
                &format!("/api/v1/pools/{pool_id}/keys"),
                Some(&host),
                &json!({"user_id": user_id}),
            ),
        )
        .await;
        keys.push((
            key["id"].as_str().unwrap().to_string(),
            key["key_prefix"].as_str().unwrap().to_string(),
        ));
    }

    // Seed usage: A×2, B×1 (the gateway writes these in production).
    let now = tributary_common::db::now_ms();
    insert_usage_event(
        db,
        &tributary_common::db::new_id(),
        now,
        &keys[0].0,
        Some(&members[0].0),
        Some(&pool_id),
        "m-test",
        10,
        5,
    )
    .await;
    insert_usage_event(
        db,
        &tributary_common::db::new_id(),
        now - 1000,
        &keys[0].0,
        Some(&members[0].0),
        Some(&pool_id),
        "m-test",
        7,
        3,
    )
    .await;
    insert_usage_event(
        db,
        &tributary_common::db::new_id(),
        now - 2000,
        &keys[1].0,
        Some(&members[1].0),
        Some(&pool_id),
        "m-test",
        100,
        50,
    )
    .await;

    Fixture {
        app,
        _host: host,
        pool_id,
        token_a: members[0].1.clone(),
        token_b: members[1].1.clone(),
        key_a: keys[0].clone(),
        key_b: keys[1].clone(),
    }
}

/// A member's key list contains ONLY their own key — same org, same pool,
/// different owner: still invisible.
#[tokio::test]
async fn member_sees_only_own_keys() {
    for db in test_dbs().await {
        let fx = provision(&db.pool).await;
        let (status, keys) = send(&fx.app, get_json("/api/v1/me/keys", Some(&fx.token_a))).await;
        assert_eq!(status, StatusCode::OK, "{}: {keys}", db.name);
        let keys = keys.as_array().unwrap();
        assert_eq!(
            keys.len(),
            1,
            "{}: member A should see exactly one key",
            db.name
        );
        assert_eq!(keys[0]["id"], fx.key_a.0);
        assert_eq!(keys[0]["key_prefix"], fx.key_a.1);
        assert_eq!(keys[0]["pool_id"], fx.pool_id);
        let surface = serde_json::to_string(&keys).unwrap();
        assert!(
            !surface.contains(&fx.key_b.0),
            "{}: member B's key id leaked to A",
            db.name
        );
        assert!(
            !surface.contains(&fx.key_b.1),
            "{}: member B's key prefix leaked to A",
            db.name
        );
    }
}

/// A member's usage shows ONLY their own rows — B's heavy usage never leaks
/// into A's table or totals.
#[tokio::test]
async fn member_usage_is_isolated() {
    for db in test_dbs().await {
        let fx = provision(&db.pool).await;

        // A: two events, 10+5+7+3 = 25 tokens total.
        let (status, usage) = send(
            &fx.app,
            get_json("/api/v1/me/usage?limit=50", Some(&fx.token_a)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}: {usage}", db.name);
        let events = usage["events"].as_array().unwrap();
        assert_eq!(events.len(), 2, "{}", db.name);
        for event in events {
            assert_eq!(event["key_prefix"], fx.key_a.1, "{}", db.name);
        }
        assert!(
            !usage.to_string().contains(&fx.key_b.1),
            "{}: B's key prefix leaked into A's usage",
            db.name
        );

        let (_, summary) = send(
            &fx.app,
            get_json("/api/v1/me/usage/summary", Some(&fx.token_a)),
        )
        .await;
        assert_eq!(summary["totals"]["requests"], 2, "{}", db.name);
        assert_eq!(summary["totals"]["tokens"], 25, "{}", db.name);
        assert!(
            !summary["days"].as_array().unwrap().is_empty(),
            "{}",
            db.name
        );

        // B: one event, 150 tokens.
        let (_, usage_b) = send(
            &fx.app,
            get_json("/api/v1/me/usage?limit=50", Some(&fx.token_b)),
        )
        .await;
        let events_b = usage_b["events"].as_array().unwrap();
        assert_eq!(events_b.len(), 1, "{}", db.name);
        let (_, summary_b) = send(
            &fx.app,
            get_json("/api/v1/me/usage/summary", Some(&fx.token_b)),
        )
        .await;
        assert_eq!(summary_b["totals"]["tokens"], 150, "{}", db.name);
    }
}

/// A member CANNOT revoke another member's key: uniform 404, B's key stays
/// active; own key revokes fine and lands in the audit trail.
#[tokio::test]
async fn member_cannot_revoke_anothers_key() {
    for db in test_dbs().await {
        let fx = provision(&db.pool).await;

        // Attempt on B's key as A → 404 (existence not even confirmed).
        let (status, _) = send(
            &fx.app,
            delete_json(
                &format!("/api/v1/me/keys/{}", fx.key_b.0),
                Some(&fx.token_a),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{}", db.name);

        // B's key is untouched.
        let (_, keys_b) = send(&fx.app, get_json("/api/v1/me/keys", Some(&fx.token_b))).await;
        assert_eq!(keys_b[0]["status"], "active", "{}", db.name);

        // A revokes their OWN key: allowed + audited (actor = A).
        let (status, _) = send(
            &fx.app,
            delete_json(
                &format!("/api/v1/me/keys/{}", fx.key_a.0),
                Some(&fx.token_a),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{}", db.name);
        let (_, keys_a) = send(&fx.app, get_json("/api/v1/me/keys", Some(&fx.token_a))).await;
        assert_eq!(keys_a[0]["status"], "revoked", "{}", db.name);

        // The admin trail records the self-revocation with A as actor.
        let host = fx._host;
        let (_, rows) = send(&fx.app, get_json("/api/v1/audit?limit=200", Some(&host))).await;
        let self_rows: Vec<&serde_json::Value> = rows
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["action"] == "pool_key.self_revoked" && r["target_id"] == fx.key_a.0)
            .collect();
        assert_eq!(self_rows.len(), 1, "{}: {rows}", db.name);
        assert!(
            self_rows[0]["actor_email"]
                .as_str()
                .unwrap()
                .starts_with("a-"),
            "{}: actor should be member A",
            db.name
        );
    }
}

/// Admin usage endpoints: pool-scoped listing + member filter + summary.
#[tokio::test]
async fn admin_usage_endpoints_scope_by_pool() {
    for db in test_dbs().await {
        let fx = provision(&db.pool).await;
        let host = fx._host;

        // Whole pool: 3 events (A's 2 + B's 1), newest first.
        let (status, usage) = send(
            &fx.app,
            get_json(
                &format!("/api/v1/usage?pool_id={}", fx.pool_id),
                Some(&host),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}: {usage}", db.name);
        assert_eq!(usage["events"].as_array().unwrap().len(), 3, "{}", db.name);

        // Member filter narrows within the pool.
        let (_, only_a) = send(
            &fx.app,
            get_json(
                &format!("/api/v1/usage?pool_id={}&user_id=member-a", fx.pool_id),
                Some(&host),
            ),
        )
        .await;
        // (member-a is not a real id — empty, not an error.)
        assert!(
            only_a["events"].as_array().unwrap().is_empty(),
            "{}: unknown user filter must yield empty",
            db.name
        );

        // Summary aggregates the pool: 3 requests, 175 tokens.
        let (status, summary) = send(
            &fx.app,
            get_json(
                &format!("/api/v1/usage/summary?pool_id={}", fx.pool_id),
                Some(&host),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}: {summary}", db.name);
        assert_eq!(summary["totals"]["requests"], 3, "{}", db.name);
        assert_eq!(summary["totals"]["tokens"], 175, "{}", db.name);

        // Unknown pool → 404 (no probing).
        let (status, _) = send(
            &fx.app,
            get_json("/api/v1/usage?pool_id=does-not-exist", Some(&host)),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{}", db.name);

        // Members cannot use the admin surface.
        let (status, _) = send(
            &fx.app,
            get_json(
                &format!("/api/v1/usage?pool_id={}", fx.pool_id),
                Some(&fx.token_a),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{}", db.name);
    }
}

/// Pre-M5 surfacing chore (workflow step 12 — public API only): the
/// member key list carries `expires_at` and the effective cap. Everything
/// with a provisioning path is provisioned through the API (org, pools,
/// members, keys incl. expiry, quota rules); only the metering rows are
/// seeded directly — the gateway's chat path is their sole writer and has
/// no control-plane provisioning surface.
///
/// Fixture shape (min-headroom proof across subjects):
/// - pool P1: key A1 (expires +3 d, KEY rule 100k/day), key B1 (no expiry);
///   POOL rule 1M/month on P1 (binds A1 + B1, never wins — huge headroom).
/// - pool P2: key A3 (already expired — issuance passes expires_at
///   through), key A4 (never expires; only a minute request-RATE rule,
///   which is not a token cap and must surface none).
/// - member B carries a USER rule 50k/day — the cross-subject leg: B1's
///   binding constraint is the user rule, not the pool rule.
/// - usage: A1 12.4k tokens today (+ one 50k row BEFORE today's window —
///   must be excluded), B1 2k today.
#[tokio::test]
async fn member_key_list_surfaces_expiry_and_effective_cap() {
    for db in test_dbs().await {
        let (app, _mock) = control_app(db.pool.clone()).await;
        let host = bootstrap_and_login(&db.pool, &app).await;
        let (_, org) = send(
            &app,
            post_json(
                "/api/v1/orgs",
                Some(&host),
                &json!({"name": "Cap", "slug": unique_slug("cap")}),
            ),
        )
        .await;
        let org_id = org["id"].as_str().unwrap().to_string();

        let mut pool_ids = Vec::new();
        for name in ["p1", "p2"] {
            let (_, pool) = send(
                &app,
                post_json(
                    "/api/v1/pools",
                    Some(&host),
                    &json!({"org_id": org_id, "name": name}),
                ),
            )
            .await;
            pool_ids.push(pool["id"].as_str().unwrap().to_string());
        }

        let mut members = Vec::new();
        for n in ["a", "b"] {
            let email = unique_email(n);
            let (_, member) = send(
                &app,
                post_json(
                    "/api/v1/members",
                    Some(&host),
                    &json!({"org_id": org_id, "email": email, "password": "password123"}),
                ),
            )
            .await;
            let (_, login) = send(
                &app,
                post_json(
                    "/api/v1/auth/login",
                    None,
                    &json!({"email": email, "password": "password123"}),
                ),
            )
            .await;
            members.push((
                member["id"].as_str().unwrap().to_string(),
                login["token"].as_str().unwrap().to_string(),
            ));
        }
        let (user_a, token_a) = (&members[0].0, &members[0].1);
        let (user_b, token_b) = (&members[1].0, &members[1].1);

        // Keys, issued through the public API with expiry bodies.
        let now = tributary_common::db::now_ms();
        let expiring_soon = now + 3 * 86_400_000; // < 7 d → UI warning tint
        let already_expired = now - 1_000;
        let (_, k_a1) = send(
            &app,
            post_json(
                &format!("/api/v1/pools/{}/keys", pool_ids[0]),
                Some(&host),
                &json!({"user_id": user_a, "expires_at": expiring_soon}),
            ),
        )
        .await;
        let (_, k_b1) = send(
            &app,
            post_json(
                &format!("/api/v1/pools/{}/keys", pool_ids[0]),
                Some(&host),
                &json!({"user_id": user_b}),
            ),
        )
        .await;
        let (_, k_a3) = send(
            &app,
            post_json(
                &format!("/api/v1/pools/{}/keys", pool_ids[1]),
                Some(&host),
                &json!({"user_id": user_a, "expires_at": already_expired}),
            ),
        )
        .await;
        let (_, k_a4) = send(
            &app,
            post_json(
                &format!("/api/v1/pools/{}/keys", pool_ids[1]),
                Some(&host),
                &json!({"user_id": user_a}),
            ),
        )
        .await;

        // Quota rules, through the public API: key-level (A1, day), a
        // user-level (B, day — the cross-subject leg), pool-level (P1,
        // month — binds A1+B1, never wins), and a request-RATE-only rule
        // (A4, minute — must NOT surface as a cap).
        let key_a1 = k_a1["id"].as_str().unwrap();
        let key_a4 = k_a4["id"].as_str().unwrap();
        for body in [
            json!({"subject_kind": "pool_key", "subject_id": key_a1,
                   "period": "day", "limit_tokens": 100_000}),
            json!({"subject_kind": "user", "subject_id": user_b,
                   "period": "day", "limit_tokens": 50_000}),
            json!({"subject_kind": "pool", "subject_id": pool_ids[0],
                   "period": "month", "limit_tokens": 1_000_000}),
            json!({"subject_kind": "pool_key", "subject_id": key_a4,
                   "period": "minute", "limit_requests": 10}),
        ] {
            let (status, body_out) =
                send(&app, post_json("/api/v1/quota-rules", Some(&host), &body)).await;
            assert_eq!(
                status,
                StatusCode::CREATED,
                "{}: {body_out} for {body}",
                db.name
            );
        }

        // Metering rows (the gateway owns this write in production): 12.4k
        // for A1 today + a 50k row from BEFORE today's window (excluded
        // from a day cap), 2k for B1 today.
        let day_start =
            tributary_common::time::period_start_ms(tributary_common::time::Period::Day, now);
        for (key, user, ts, tokens) in [
            (&k_a1, user_a, now, 10_000i64),
            (&k_a1, user_a, now, 2_400),
            (&k_a1, user_a, day_start - 1, 50_000),
            (&k_b1, user_b, now, 2_000),
        ] {
            insert_usage_event(
                &db.pool,
                &tributary_common::db::new_id(),
                ts,
                key["id"].as_str().unwrap(),
                Some(user),
                Some(&pool_ids[0]),
                "m-test",
                tokens,
                0,
            )
            .await;
        }

        // Member A: three keys, exact expiry + cap values.
        let (status, keys_a) = send(&app, get_json("/api/v1/me/keys", Some(token_a))).await;
        assert_eq!(status, StatusCode::OK, "{}: {keys_a}", db.name);
        let keys_a = keys_a.as_array().unwrap();
        assert_eq!(keys_a.len(), 3, "{}", db.name);
        // A nested fn (not a closure): closures cannot express that the
        // returned reference borrows from `keys` — a named lifetime can.
        fn by_id<'a>(
            keys: &'a [serde_json::Value],
            id: &str,
            db_name: &str,
        ) -> &'a serde_json::Value {
            keys.iter()
                .find(|k| k["id"] == id)
                .unwrap_or_else(|| panic!("{db_name}: key {id} missing"))
        }
        let a1 = by_id(keys_a, k_a1["id"].as_str().unwrap(), db.name);
        assert_eq!(a1["expires_at"], expiring_soon, "{}", db.name);
        assert_eq!(a1["cap"]["limit_tokens"], 100_000, "{}: {a1}", db.name);
        assert_eq!(a1["cap"]["period"], "day", "{}: {a1}", db.name);
        // 12.4k counted, the pre-window 50k row excluded.
        assert_eq!(a1["cap"]["used_tokens"], 12_400, "{}: {a1}", db.name);
        let a3 = by_id(keys_a, k_a3["id"].as_str().unwrap(), db.name);
        assert_eq!(a3["expires_at"], already_expired, "{}: {a3}", db.name);
        // Display-level expiry only — the DB status stays "active".
        assert_eq!(a3["status"], "active", "{}: {a3}", db.name);
        // No token rule binds A3 (its pool has none) → null cap = Unlimited.
        assert_eq!(a3["cap"], serde_json::Value::Null, "{}: {a3}", db.name);
        let a4 = by_id(keys_a, k_a4["id"].as_str().unwrap(), db.name);
        assert_eq!(a4["expires_at"], serde_json::Value::Null, "{}", db.name);
        // A request-RATE rule is not a token cap — still Unlimited.
        assert_eq!(a4["cap"], serde_json::Value::Null, "{}: {a4}", db.name);

        // Member B: one key; the USER rule (50k day, 2k used) beats the
        // pool-wide 1M month rule — the cross-subject min-headroom leg.
        let (status, keys_b) = send(&app, get_json("/api/v1/me/keys", Some(token_b))).await;
        assert_eq!(status, StatusCode::OK, "{}: {keys_b}", db.name);
        let keys_b = keys_b.as_array().unwrap();
        assert_eq!(keys_b.len(), 1, "{}", db.name);
        assert_eq!(keys_b[0]["id"], k_b1["id"], "{}", db.name);
        assert_eq!(
            keys_b[0]["expires_at"],
            serde_json::Value::Null,
            "{}",
            db.name
        );
        assert_eq!(keys_b[0]["cap"]["limit_tokens"], 50_000, "{}", db.name);
        assert_eq!(keys_b[0]["cap"]["period"], "day", "{}", db.name);
        assert_eq!(keys_b[0]["cap"]["used_tokens"], 2_000, "{}", db.name);

        // Masking + isolation invariants on the new surface (rule 2): no
        // raw key material, nothing of the other member's.
        let surface_a = serde_json::to_string(&keys_a).unwrap();
        for raw in [&k_a1, &k_b1, &k_a3, &k_a4] {
            let raw_str = raw["raw"].as_str().unwrap();
            assert!(
                !surface_a.contains(raw_str),
                "{}: raw key material leaked to the member list",
                db.name
            );
        }
        assert!(
            !surface_a.contains(k_b1["id"].as_str().unwrap())
                && !surface_a.contains(k_b1["key_prefix"].as_str().unwrap()),
            "{}: member B's key leaked into A's list",
            db.name
        );
        let surface_b = serde_json::to_string(&keys_b).unwrap();
        assert!(
            !surface_b.contains(k_a1["key_prefix"].as_str().unwrap()),
            "{}: member A's key prefix leaked into B's list",
            db.name
        );
    }
}
