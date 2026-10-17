//! Audit-log integration tests (M3): every control-plane mutation leaves a
//! scoped trail, and the trail NEVER contains key material (director
//! directive 2026-10-17 — the sanitization invariant is proven by scanning
//! every column of every row after issuing real secrets through the API).

mod harness;

use axum::http::StatusCode;
use serde_json::json;

use harness::*;

/// Walks the full mutation surface, then asserts each action landed in the
/// audit trail with the right actor and masked snapshots.
#[tokio::test]
async fn audit_rows_written_on_control_plane_mutations() {
    for db in test_dbs().await {
        let (app, mock) = control_app(db.pool.clone()).await;
        let host = bootstrap_and_login(&db.pool, &app).await;

        // org.created
        let (_, org) = send(
            &app,
            post_json(
                "/api/v1/orgs",
                Some(&host),
                &json!({"name": "AuditCo", "slug": unique_slug("audit")}),
            ),
        )
        .await;
        let org_id = org["id"].as_str().unwrap().to_string();

        // pool.created
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

        // member.created
        let (_, member) = send(
            &app,
            post_json(
                "/api/v1/members",
                Some(&host),
                &json!({"org_id": org_id, "email": unique_email("mem"), "password": "password123"}),
            ),
        )
        .await;
        let member_id = member["id"].as_str().unwrap().to_string();

        // pool_key.issued
        let (_, key) = send(
            &app,
            post_json(
                &format!("/api/v1/pools/{pool_id}/keys"),
                Some(&host),
                &json!({"user_id": member_id}),
            ),
        )
        .await;
        let key_id = key["id"].as_str().unwrap().to_string();

        // pool_key.revoked
        let (status, _) = send(
            &app,
            delete_json(
                &format!("/api/v1/pools/{pool_id}/keys/{key_id}"),
                Some(&host),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{}", db.name);

        // provider_key.created (+ revalidated + revoked)
        let (_, pk) = send(
            &app,
            post_json(
                &format!("/api/v1/pools/{pool_id}/provider-keys"),
                Some(&host),
                &json!({"key": UPSTREAM_KEY, "base_url": mock.base_url}),
            ),
        )
        .await;
        let pk_id = pk["id"].as_str().unwrap().to_string();
        send(
            &app,
            post_json(
                &format!("/api/v1/pools/{pool_id}/provider-keys/{pk_id}/revalidate"),
                Some(&host),
                &json!({}),
            ),
        )
        .await;
        send(
            &app,
            delete_json(
                &format!("/api/v1/pools/{pool_id}/provider-keys/{pk_id}"),
                Some(&host),
            ),
        )
        .await;

        // quota_rule.upserted (+ deleted)
        let (_, rule) = send(
            &app,
            post_json(
                "/api/v1/quota-rules",
                Some(&host),
                &json!({"subject_kind": "pool", "subject_id": pool_id, "period": "day", "limit_tokens": 1000}),
            ),
        )
        .await;
        send(
            &app,
            delete_json(
                &format!("/api/v1/quota-rules/{}", rule["id"].as_str().unwrap()),
                Some(&host),
            ),
        )
        .await;

        // The trail (host sees everything, newest first).
        let (status, rows) = send(&app, get_json("/api/v1/audit?limit=200", Some(&host))).await;
        assert_eq!(status, StatusCode::OK, "{}", db.name);
        let actions: Vec<&str> = rows
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["action"].as_str().unwrap())
            .collect();
        for expected in [
            "org.created",
            "pool.created",
            "member.created",
            "pool_key.issued",
            "pool_key.revoked",
            "provider_key.created",
            "provider_key.revalidated",
            "provider_key.revoked",
            "quota_rule.upserted",
            "quota_rule.deleted",
            "session.opened", // the bootstrap login itself is audited
        ] {
            assert!(
                actions.contains(&expected),
                "{}: audit trail missing '{expected}'; got {actions:?}",
                db.name
            );
        }
        // Rows carry actor identity + org scope + target linkage. (Filter by
        // target_id too: on the shared PG database, parallel tests write
        // identical action names.)
        let issued = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["action"] == "pool_key.issued" && r["target_id"] == key_id)
            .unwrap();
        assert_eq!(issued["target_kind"], "pool_key");
        assert_eq!(issued["target_id"], key_id);
        assert_eq!(issued["org_id"], org_id);
        assert!(issued["actor_email"].as_str().unwrap().contains('@'));
    }
}

/// THE sanitization proof: after pushing a real pool key and a real provider
/// key through the API, NO column of ANY audit row contains the raw secret,
/// a key hash, ciphertext, or a password hash.
#[tokio::test]
async fn audit_log_never_contains_key_material() {
    for db in test_dbs().await {
        let (app, mock) = control_app(db.pool.clone()).await;
        let host = bootstrap_and_login(&db.pool, &app).await;
        let (_, org) = send(
            &app,
            post_json(
                "/api/v1/orgs",
                Some(&host),
                &json!({"name": "Sanitize", "slug": unique_slug("san")}),
            ),
        )
        .await;
        let org_id = org["id"].as_str().unwrap().to_string();
        let (_, pool) = send(
            &app,
            post_json(
                "/api/v1/pools",
                Some(&host),
                &json!({"org_id": org_id, "name": "p"}),
            ),
        )
        .await;
        let pool_id = pool["id"].as_str().unwrap().to_string();
        let (_, member) = send(
            &app,
            post_json(
                "/api/v1/members",
                Some(&host),
                &json!({"org_id": org_id, "email": unique_email("mem"), "password": "super-secret-passw0rd"}),
            ),
        )
        .await;

        // Issue a pool key: `raw` exists exactly here, in the response.
        let (_, key) = send(
            &app,
            post_json(
                &format!("/api/v1/pools/{pool_id}/keys"),
                Some(&host),
                &json!({"user_id": member["id"].as_str().unwrap()}),
            ),
        )
        .await;
        let raw_pool_key = key["raw"].as_str().unwrap().to_string();

        // Add a provider key with a known plaintext.
        send(
            &app,
            post_json(
                &format!("/api/v1/pools/{pool_id}/provider-keys"),
                Some(&host),
                &json!({"key": UPSTREAM_KEY, "base_url": mock.base_url}),
            ),
        )
        .await;

        // Also compute what a leaked hash would look like (defense against
        // "we only logged the hash" regressions).
        let raw_hash = tributary_common::keys::hash_token(&raw_pool_key);

        // Scan EVERY column of EVERY row (API output AND storage form).
        let (_, rows) = send(&app, get_json("/api/v1/audit?limit=200", Some(&host))).await;
        let api_surface = rows.to_string();
        let stored = tributary_control::audit::raw_rows_for_test(&db.pool).await;
        assert!(!stored.is_empty(), "{}: expected audit rows", db.name);
        let stored_surface = stored.join("\n");

        for surface in [api_surface.clone(), stored_surface] {
            assert!(
                !surface.contains(&raw_pool_key),
                "{}: raw pool key leaked into audit_log",
                db.name
            );
            assert!(
                !surface.contains(&raw_hash),
                "{}: pool-key HASH leaked into audit_log",
                db.name
            );
            assert!(
                !surface.contains(UPSTREAM_KEY),
                "{}: raw provider key leaked into audit_log",
                db.name
            );
            assert!(
                !surface.contains("super-secret-passw0rd"),
                "{}: plaintext password leaked into audit_log",
                db.name
            );
            assert!(
                !surface.contains("key_hash"),
                "{}: key_hash field name present in audit_log",
                db.name
            );
            assert!(
                !surface.contains("ciphertext"),
                "{}: ciphertext field name present in audit_log",
                db.name
            );
            assert!(
                !surface.contains("password_hash"),
                "{}: password_hash field name present in audit_log",
                db.name
            );
        }

        // Positive control: the MASKED display forms ARE there (prefix +
        // last-4 only — the trail stays useful).
        assert!(
            api_surface.contains(key["key_prefix"].as_str().unwrap()),
            "{}: masked pool-key prefix missing from audit trail",
            db.name
        );
    }
}

/// org_admin sees only their org's trail; members get nothing (403).
#[tokio::test]
async fn audit_visibility_scopes_by_org() {
    for db in test_dbs().await {
        let (app, _mock) = control_app(db.pool.clone()).await;
        let host = bootstrap_and_login(&db.pool, &app).await;

        // Two orgs, one pool each.
        let mut org_ids = Vec::new();
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
            org_ids.push(org["id"].as_str().unwrap().to_string());
            pool_ids.push(pool["id"].as_str().unwrap().to_string());
        }

        // An org_admin for org A (seeded directly — admin provisioning UI
        // stays host-side in M3) and a plain member of org A.
        let admin_a = tributary_control::db::UserRow {
            id: tributary_common::db::new_id(),
            email: unique_email("admin-a"),
            password_hash: tributary_control::auth::hash_password("password123").unwrap(),
            role: "org_admin".into(),
            org_id: Some(org_ids[0].clone()),
            created_at: tributary_common::db::now_ms(),
        };
        tributary_control::db::insert_user(&db.pool, &admin_a)
            .await
            .unwrap();
        let member = tributary_control::db::UserRow {
            id: tributary_common::db::new_id(),
            email: unique_email("mem-a"),
            password_hash: tributary_control::auth::hash_password("password123").unwrap(),
            role: "member".into(),
            org_id: Some(org_ids[0].clone()),
            created_at: tributary_common::db::now_ms(),
        };
        tributary_control::db::insert_user(&db.pool, &member)
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
        let (_, login) = send(
            &app,
            post_json(
                "/api/v1/auth/login",
                None,
                &json!({"email": member.email, "password": "password123"}),
            ),
        )
        .await;
        let token_member = login["token"].as_str().unwrap().to_string();

        // Mutations in org B (member created + key issued) — rows recorded
        // against org B must be invisible to A.
        let (_, member_b) = send(
            &app,
            post_json(
                "/api/v1/members",
                Some(&host),
                &json!({"org_id": org_ids[1], "email": unique_email("mem-b"), "password": "password123"}),
            ),
        )
        .await;
        send(
            &app,
            post_json(
                &format!("/api/v1/pools/{}/keys", pool_ids[1]),
                Some(&host),
                &json!({"user_id": member_b["id"].as_str().unwrap()}),
            ),
        )
        .await;

        // org A admin: sees their org's rows (incl. their own login), never
        // org B's.
        let (status, rows) = send(&app, get_json("/api/v1/audit?limit=200", Some(&token_a))).await;
        assert_eq!(status, StatusCode::OK, "{}", db.name);
        let rows = rows.as_array().unwrap();
        assert!(
            !rows.is_empty(),
            "{}: org admin should see their org rows",
            db.name
        );
        for row in rows {
            assert_eq!(
                row["org_id"], org_ids[0],
                "{}: cross-org row leaked to org admin: {row}",
                db.name
            );
        }

        // Member: the audit viewer is admin tooling — 403.
        let (status, _) = send(&app, get_json("/api/v1/audit", Some(&token_member))).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{}", db.name);

        // Unauthenticated: 401.
        let (status, _) = send(&app, get_json("/api/v1/audit", None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{}", db.name);
    }
}
