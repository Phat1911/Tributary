//! Streaming integration tests: SSE passthrough, the include_usage
//! inject-and-strip contract, and the chunk-count fallback.

mod common;

use axum::http::header;
use common::*;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn stream_body() -> serde_json::Value {
    let mut body = chat_body();
    body["stream"] = serde_json::Value::Bool(true);
    body
}

#[tokio::test]
async fn gateway_injected_usage_is_stripped_downstream_but_metered() {
    for db in test_dbs().await {
        let mock = spawn_mock(MockMode::Stream { with_usage: true }).await;
        let app = test_app(db.pool.clone(), &mock).await;
        let (raw, id) = seed_key(&db.pool, "active", None, None).await;

        // Client did NOT set include_usage — gateway injects it upstream.
        let response = app
            .oneshot(chat_request(Some(&raw), &stream_body()))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("text/event-stream")
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(
            text.contains("Hel") && text.contains("lo"),
            "content chunks pass through"
        );
        assert!(text.contains("[DONE]"), "terminal event passes through");
        assert!(
            !text.contains("usage"),
            "injected usage-only chunk must be stripped downstream:\n{text}"
        );

        // Upstream request carried stream_options.include_usage = true.
        {
            let captured = mock.captured.lock().unwrap();
            assert_eq!(
                captured[0].body["stream_options"]["include_usage"], true,
                "gateway must ask upstream for usage"
            );
        }

        // Exact usage metered from the stripped chunk.
        let rows = wait_for_usage(&db.pool, &id, 1).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].prompt_tokens, 9);
        assert_eq!(rows[0].completion_tokens, 3);
        assert_eq!(rows[0].model, "m-test");
        assert_eq!(rows[0].status, "200");
    }
}

#[tokio::test]
async fn client_requested_usage_chunk_is_forwarded_untouched() {
    for db in test_dbs().await {
        let mock = spawn_mock(MockMode::Stream { with_usage: true }).await;
        let app = test_app(db.pool.clone(), &mock).await;
        let (raw, id) = seed_key(&db.pool, "active", None, None).await;

        // Client asks for usage itself — every byte must be forwarded.
        let mut body = stream_body();
        body["stream_options"]["include_usage"] = serde_json::Value::Bool(true);
        let response = app.oneshot(chat_request(Some(&raw), &body)).await.unwrap();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(
            text.contains("\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":3}"),
            "client-requested usage chunk must survive passthrough:\n{text}"
        );
        assert!(text.contains("[DONE]"));

        {
            let captured = mock.captured.lock().unwrap();
            assert_eq!(captured[0].body["stream_options"]["include_usage"], true);
        }

        let rows = wait_for_usage(&db.pool, &id, 1).await;
        assert_eq!(rows[0].prompt_tokens, 9);
        assert_eq!(rows[0].completion_tokens, 3);
    }
}

#[tokio::test]
async fn fallback_counts_chunks_when_upstream_sends_no_usage() {
    for db in test_dbs().await {
        let mock = spawn_mock(MockMode::Stream { with_usage: false }).await;
        let app = test_app(db.pool.clone(), &mock).await;
        let (raw, id) = seed_key(&db.pool, "active", None, None).await;

        let response = app
            .oneshot(chat_request(Some(&raw), &stream_body()))
            .await
            .unwrap();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("[DONE]"));
        assert!(!text.contains("usage"));

        // Approximate metering: two content chunks counted, prompt unknown.
        let rows = wait_for_usage(&db.pool, &id, 1).await;
        assert_eq!(
            rows[0].completion_tokens, 2,
            "one token per content chunk (approx)"
        );
        assert_eq!(rows[0].prompt_tokens, 0);
        assert_eq!(rows[0].model, "m-test");
    }
}
