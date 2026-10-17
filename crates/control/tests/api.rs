//! Control-plane integration tests: dual-DB (SQLite tempfile + Postgres via
//! env) against an in-process axum mock upstream for validation probes.

#![allow(dead_code)] // shared helpers, not all used by every test

mod harness;

use axum::http::StatusCode;
use serde_json::json;

use harness::*;

#[tokio::test] // tokio: run this async fn on a test runtime (each test gets one)
async fn bootstrap_login_and_reject_bad_credentials() {
    for db in test_dbs().await {
        let (app, _mock) = control_app(db.pool.clone()).await;
        tributary_control::create_host_admin(&db.pool, "root@tributary.dev", "hunter2hunter2")
            .await
            .expect("bootstrap");

        // Second bootstrap must be refused (CLI cannot mint more admins).
        assert!(
            tributary_control::create_host_admin(&db.pool, "other@tributary.dev", "hunter2hunter2")
                .await
                .is_err()
        );

        // Good credentials → token.
        let (status, body) = send(
            &app,
            post_json(
                "/api/v1/auth/login",
                None,
                &json!({"email": "root@tributary.dev", "password": "hunter2hunter2"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", db.name);
        assert!(body["token"].as_str().unwrap().starts_with("ses-"));

        // Wrong password and unknown email → identical 401 (no probing).
        for creds in [
            json!({"email": "root@tributary.dev", "password": "wrong-password"}),
            json!({"email": "ghost@tributary.dev", "password": "hunter2hunter2"}),
        ] {
            let (status, err) = send(&app, post_json("/api/v1/auth/login", None, &creds)).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{}: {err}", db.name);
        }
    }
}

#[tokio::test]
async fn unauthenticated_calls_are_rejected() {
    for db in test_dbs().await {
        let (app, _mock) = control_app(db.pool.clone()).await;
        let (status, _) = send(&app, get_json("/api/v1/orgs", None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{}", db.name);
    }
}

#[tokio::test]
async fn org_pool_provider_key_happy_path() {
    for db in test_dbs().await {
        let (app, mock) = control_app(db.pool.clone()).await;
        let token = bootstrap_and_login(&db.pool, &app).await;

        // Org → pool (default allowlist) → provider key validated by the mock.
        let (_, org) = send(
            &app,
            post_json(
                "/api/v1/orgs",
                Some(&token),
                &json!({"name": "Acme", "slug": unique_slug("acme")}),
            ),
        )
        .await;
        assert!(org["slug"].as_str().unwrap().starts_with("acme-"));

        let (_, pool) = send(
            &app,
            post_json(
                "/api/v1/pools",
                Some(&token),
                &json!({"org_id": org["id"].as_str().unwrap(), "name": "main"}),
            ),
        )
        .await;
        assert_eq!(pool["model_allowlist"][0], "m-test", "default models apply");

        let (status, pk) = send(
            &app,
            post_json(
                &format!(
                    "/api/v1/pools/{}/provider-keys",
                    pool["id"].as_str().unwrap()
                ),
                Some(&token),
                &json!({"key": UPSTREAM_KEY, "base_url": mock.base_url}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{}: {pk}", db.name);
        assert_eq!(pk["health"], "ok", "mock accepts the right key");
        assert!(pk["key_prefix"].as_str().unwrap().contains('…'));

        // The stored row is ciphertext, never the raw key (rule 2).
        let rows =
            tributary_control::db::provider_keys_by_pool(&db.pool, pool["id"].as_str().unwrap())
                .await
                .expect("rows");
        assert_eq!(rows.len(), 1);
        assert_ne!(rows[0].ciphertext, UPSTREAM_KEY);
        assert!(!rows[0].ciphertext.contains("SECRET"));
        assert!(!rows[0].dek_wrapped.contains("SECRET"));

        // Listing is masked: no ciphertext fields at all.
        let (status, list) = send(
            &app,
            get_json(
                &format!(
                    "/api/v1/pools/{}/provider-keys",
                    pool["id"].as_str().unwrap()
                ),
                Some(&token),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let list_str = list.to_string();
        assert!(
            !list_str.contains("ciphertext"),
            "no ciphertext in API output"
        );
        assert!(!list_str.contains(UPSTREAM_KEY), "no raw key in API output");
        assert!(!list_str.contains("dek_wrapped"));
    }
}

#[tokio::test]
async fn provider_key_with_wrong_secret_is_flagged_invalid() {
    for db in test_dbs().await {
        let (app, mock) = control_app(db.pool.clone()).await;
        let token = bootstrap_and_login(&db.pool, &app).await;
        let (_, org) = send(
            &app,
            post_json(
                "/api/v1/orgs",
                Some(&token),
                &json!({"name": "B", "slug": unique_slug("b")}),
            ),
        )
        .await;
        let (_, pool) = send(
            &app,
            post_json(
                "/api/v1/pools",
                Some(&token),
                &json!({"org_id": org["id"].as_str().unwrap(), "name": "p"}),
            ),
        )
        .await;

        // The mock 401s any bearer that is not UPSTREAM_KEY.
        let (status, pk) = send(
            &app,
            post_json(
                &format!(
                    "/api/v1/pools/{}/provider-keys",
                    pool["id"].as_str().unwrap()
                ),
                Some(&token),
                &json!({"key": "sk-this-is-not-valid", "base_url": mock.base_url}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(pk["health"], "invalid", "{}", db.name);

        // Manual revalidate keeps the verdict.
        let (_, again) = send(
            &app,
            post_json(
                &format!(
                    "/api/v1/pools/{}/provider-keys/{}/revalidate",
                    pool["id"].as_str().unwrap(),
                    pk["id"].as_str().unwrap()
                ),
                Some(&token),
                &json!({}),
            ),
        )
        .await;
        assert_eq!(again["health"], "invalid");
    }
}

#[tokio::test]
async fn pool_key_issuance_reveals_raw_once_and_masks_after() {
    for db in test_dbs().await {
        let (app, _mock) = control_app(db.pool.clone()).await;
        let token = bootstrap_and_login(&db.pool, &app).await;
        let (_, org) = send(
            &app,
            post_json(
                "/api/v1/orgs",
                Some(&token),
                &json!({"name": "C", "slug": unique_slug("c")}),
            ),
        )
        .await;
        let (_, pool) = send(
            &app,
            post_json(
                "/api/v1/pools",
                Some(&token),
                &json!({"org_id": org["id"].as_str().unwrap(), "name": "p"}),
            ),
        )
        .await;
        let (_, member) = send(
            &app,
            post_json(
                "/api/v1/members",
                Some(&token),
                &json!({"org_id": org["id"].as_str().unwrap(), "email": unique_email("mem"), "password": "password123"}),
            ),
        )
        .await;

        let (status, key) = send(
            &app,
            post_json(
                &format!("/api/v1/pools/{}/keys", pool["id"].as_str().unwrap()),
                Some(&token),
                &json!({"user_id": member["id"].as_str().unwrap()}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{}", db.name);
        assert!(key["raw"].as_str().unwrap().starts_with("sk-pool-"));

        // Listing never shows the raw key or its hash.
        let (_, list) = send(
            &app,
            get_json(
                &format!("/api/v1/pools/{}/keys", pool["id"].as_str().unwrap()),
                Some(&token),
            ),
        )
        .await;
        let list_str = list.to_string();
        assert!(!list_str.contains(key["raw"].as_str().unwrap()));
        assert!(!list_str.contains("key_hash"));

        // Revocation flips status.
        let (status, _) = send(
            &app,
            axum::http::Request::builder()
                .method("DELETE")
                .uri(format!(
                    "/api/v1/pools/{}/keys/{}",
                    pool["id"].as_str().unwrap(),
                    key["id"].as_str().unwrap()
                ))
                .header("authorization", format!("Bearer {token}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, list) = send(
            &app,
            get_json(
                &format!("/api/v1/pools/{}/keys", pool["id"].as_str().unwrap()),
                Some(&token),
            ),
        )
        .await;
        assert_eq!(list[0]["status"], "revoked");
    }
}

#[tokio::test]
async fn org_admin_sees_only_their_org() {
    for db in test_dbs().await {
        let (app, mock) = control_app(db.pool.clone()).await;
        let host = bootstrap_and_login(&db.pool, &app).await;

        // Two orgs, two pools.
        let mut pool_ids = Vec::new();
        for n in ["a", "b"] {
            let (_, org) = send(
                &app,
                post_json(
                    "/api/v1/orgs",
                    Some(&host),
                    &json!({"name": n, "slug": unique_slug(n)}),
                ),
            )
            .await;
            let (_, pool) = send(
                &app,
                post_json(
                    "/api/v1/pools",
                    Some(&host),
                    &json!({"org_id": org["id"].as_str().unwrap(), "name": n}),
                ),
            )
            .await;
            pool_ids.push((
                org["id"].as_str().unwrap().to_string(),
                pool["id"].as_str().unwrap().to_string(),
            ));
        }
        let (org_a, pool_a) = (&pool_ids[0].0, &pool_ids[0].1);
        let (_org_b, pool_b) = (&pool_ids[1].0, &pool_ids[1].1);
        let _ = mock.base_url; // mock unused here; silence warning

        // Seed an org_admin for org A directly (admin provisioning UI = M3).
        let admin_a = tributary_control::db::UserRow {
            id: tributary_common::db::new_id(),
            email: unique_email("admin-a"),
            password_hash: tributary_control::auth::hash_password("password123").unwrap(),
            role: "org_admin".into(),
            org_id: Some(org_a.clone()),
            created_at: tributary_common::db::now_ms(),
        };
        tributary_control::db::insert_user(&db.pool, &admin_a)
            .await
            .unwrap();
        let (_, login) = send(
            &app,
            post_json(
                "/api/v1/auth/login",
                None,
                &json!({"email": admin_a.email, "password": "password123"}),
            ),
        )
        .await;
        let token_a = login["token"].as_str().unwrap().to_string();

        // Own org's pool visible; other org's pool hidden as 404 (no probing).
        let (status, _) = send(
            &app,
            get_json(&format!("/api/v1/pools/{pool_a}"), Some(&token_a)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", db.name);
        let (status, _) = send(
            &app,
            get_json(&format!("/api/v1/pools/{pool_b}"), Some(&token_a)),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{}", db.name);

        // Cannot create a member in the other org either.
        let (status, _) = send(
            &app,
            post_json(
                "/api/v1/members",
                Some(&token_a),
                &json!({"org_id": pool_ids[1].0.clone(), "email": unique_email("x"), "password": "password123"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // Listing shows only org A's pool.
        let (_, pools) = send(&app, get_json("/api/v1/pools", Some(&token_a))).await;
        assert_eq!(pools.as_array().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn quota_rules_upsert_and_scoped_visibility() {
    for db in test_dbs().await {
        let (app, _mock) = control_app(db.pool.clone()).await;
        let host = bootstrap_and_login(&db.pool, &app).await;
        let (_, org) = send(
            &app,
            post_json(
                "/api/v1/orgs",
                Some(&host),
                &json!({"name": "Q", "slug": unique_slug("q")}),
            ),
        )
        .await;
        let (_, pool) = send(
            &app,
            post_json(
                "/api/v1/pools",
                Some(&host),
                &json!({"org_id": org["id"].as_str().unwrap(), "name": "p"}),
            ),
        )
        .await;
        let pool_id = pool["id"].as_str().unwrap();

        // Upsert creates, then replaces (one row per subject+period).
        for limit in [100, 50] {
            let (status, rule) = send(
                &app,
                post_json(
                    "/api/v1/quota-rules",
                    Some(&host),
                    &json!({"subject_kind": "pool", "subject_id": pool_id, "period": "day", "limit_tokens": limit}),
                ),
            )
            .await;
            assert_eq!(status, StatusCode::CREATED, "{}: {rule}", db.name);
        }
        let (_, rules) = send(&app, get_json("/api/v1/quota-rules", Some(&host))).await;
        let day_rules = rules
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["period"] == "day" && r["subject_id"] == *pool_id)
            .count();
        assert_eq!(day_rules, 1, "upsert replaced, not appended");

        // A rate rule on the same subject is a different period → coexists.
        let (status, _) = send(
            &app,
            post_json(
                "/api/v1/quota-rules",
                Some(&host),
                &json!({"subject_kind": "pool", "subject_id": pool_id, "period": "minute", "limit_requests": 3}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        // Bad input rejected.
        for bad in [
            json!({"subject_kind": "galaxy", "subject_id": pool_id, "period": "day", "limit_tokens": 1}),
            json!({"subject_kind": "pool", "subject_id": pool_id, "period": "fortnight", "limit_tokens": 1}),
            json!({"subject_kind": "pool", "subject_id": pool_id, "period": "day"}),
        ] {
            let (status, _) = send(&app, post_json("/api/v1/quota-rules", Some(&host), &bad)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{}: {bad}", db.name);
        }
    }
}
