//! Data-plane integration tests: auth, non-streaming proxy, error passthrough,
//! /v1/models, and secret hygiene. Runs against SQLite and (when configured)
//! Postgres, always through the mocked upstream.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::*;
use http_body_util::BodyExt;
use tower::ServiceExt;
use tributary_gateway::{db, keys};

#[tokio::test]
async fn auth_rejects_missing_unknown_revoked_expired_keys() {
    for db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(db.pool.clone(), &mock).await;
        let (_raw, id) = seed_key(&db.pool, None, "active", None, None).await;
        let (revoked_raw, _revoked_id) = seed_key(&db.pool, None, "revoked", None, None).await;
        let (expired_raw, _expired_id) =
            seed_key(&db.pool, None, "active", Some(db::now_ms() - 1_000), None).await;

        let cases = [
            (None, "missing header"),
            (
                Some("sk-pool-doesnotexist0000000000000000000000000000000000000000000000"),
                "unknown key",
            ),
            (Some("not-a-pool-key"), "wrong scheme token"),
            (Some(&revoked_raw), "revoked key"),
            (Some(&expired_raw), "expired key"),
        ];
        for (auth, label) in cases {
            let response = app
                .clone()
                .oneshot(chat_request(auth, &chat_body()))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "{label} / {}",
                db.name
            );
            let body = response.into_body().collect().await.unwrap().to_bytes();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(
                body["error"]["code"], "invalid_api_key",
                "{label} / {}",
                db.name
            );
        }

        // No usage rows for rejected requests.
        let rows = db::usage_events_for_key(&db.pool, &id).await.unwrap();
        assert!(
            rows.is_empty(),
            "auth failures must not be metered / {}",
            db.name
        );
        assert!(
            mock.captured.lock().unwrap().is_empty(),
            "rejected requests must never reach upstream"
        );
    }
}

#[tokio::test]
async fn non_streaming_proxies_and_meters() {
    for db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(db.pool.clone(), &mock).await;
        let setup = setup_working(&db.pool, &mock).await;
        let (raw, id) = (setup.key_raw.clone(), setup.key_id.clone());

        let response = app
            .oneshot(chat_request(Some(&raw), &chat_body()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("application/json")
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["choices"][0]["message"]["content"], "Hello!");
        assert_eq!(body["usage"]["prompt_tokens"], 11);

        // Upstream got the provider key, not the pool key; body untouched.
        {
            let captured = mock.captured.lock().unwrap();
            assert_eq!(captured.len(), 1);
            assert_eq!(
                captured[0].auth.as_deref(),
                Some("Bearer sk-test-upstream-key")
            );
            assert_eq!(captured[0].body["model"], "m-test");
            assert!(captured[0].body.get("stream_options").is_none());
        }

        // Usage row written with exact tokens.
        let rows = wait_for_usage(&db.pool, &id, 1).await;
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.pool_key_id, id);
        assert_eq!(row.user_id.as_deref(), Some("user-1"));
        assert_eq!(row.model, "m-test");
        assert_eq!(row.prompt_tokens, 11);
        assert_eq!(row.completion_tokens, 7);
        assert_eq!(row.cost_micros, 0, "cost stays 0 until pricing (D23)");
        assert_eq!(row.status, "200");
        assert_eq!(row.pool_id.as_deref(), Some(setup.pool_id.as_str()));
        assert_eq!(
            row.provider_key_id.as_deref(),
            Some(setup.provider_key_id.as_str())
        );
    }
}

#[tokio::test]
async fn malformed_body_is_rejected_openai_style() {
    for db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(db.pool.clone(), &mock).await;
        let setup = setup_working(&db.pool, &mock).await;
        let raw = setup.key_raw.clone();

        let request = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header(header::AUTHORIZATION, format!("Bearer {raw}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("this is not json"))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["type"], "invalid_request_error");
        assert!(mock.captured.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn upstream_error_passes_through_and_is_metered() {
    for db in test_dbs().await {
        let mock = spawn_mock(MockMode::Error(500)).await;
        let app = test_app(db.pool.clone(), &mock).await;
        let setup = setup_working(&db.pool, &mock).await;
        let (raw, id) = (setup.key_raw.clone(), setup.key_id.clone());

        let response = app
            .oneshot(chat_request(Some(&raw), &chat_body()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["message"], "mock boom");

        let rows = wait_for_usage(&db.pool, &id, 1).await;
        assert_eq!(rows[0].status, "500");
        assert_eq!(rows[0].prompt_tokens, 0);
        assert_eq!(rows[0].completion_tokens, 0);
    }
}

#[tokio::test]
async fn models_lists_config_and_requires_auth() {
    for db in test_dbs().await {
        let mock = spawn_mock(MockMode::NonStream).await;
        let app = test_app(db.pool.clone(), &mock).await;
        let setup = setup_working(&db.pool, &mock).await;
        let raw = setup.key_raw.clone();

        let unauth = Request::builder()
            .uri("/v1/models")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(unauth).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let authed = Request::builder()
            .uri("/v1/models")
            .header(header::AUTHORIZATION, format!("Bearer {raw}"))
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(authed).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["object"], "list");
        let ids: Vec<&str> = body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["m-test", "m-other"]);
    }
}

#[tokio::test]
async fn raw_key_material_never_lands_in_storage() {
    for db in test_dbs().await {
        let (raw, _id) = seed_key(&db.pool, None, "active", None, None).await;
        let hash = keys::hash_token(&raw);
        let row = db::lookup_pool_key(&db.pool, &hash).await.unwrap().unwrap();
        // No stored field contains the raw key or its random body.
        let random_body = raw.trim_start_matches("sk-pool-");
        let random_body = &random_body[..16]; // any 16 chars would identify it
        for field in [&row.key_hash, &row.key_prefix, &row.kind, &row.status] {
            assert!(!field.contains(random_body), "leak in {field:?}");
            assert!(!field.contains(&raw));
        }
    }
}
