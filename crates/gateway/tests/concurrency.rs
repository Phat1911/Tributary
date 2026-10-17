//! Concurrent-writer regression (director ruling 2026-10-16): with WAL +
//! `busy_timeout`, simultaneous metering inserts must queue instead of
//! failing `SQLITE_BUSY` — a failed insert is a silently unmetered request
//! (AUDIT A-003-adjacent).

mod common;

use common::{seed_key, test_dbs};
use tributary_gateway::db;

/// 8 tasks × 5 metering inserts each, hammering the same database at once.
///
/// Every insert must succeed and every row must land exactly once. Runs on
/// the full dual-DB matrix; on the single-connection in-memory pool used by
/// Windows-local runs it passes by construction (writes serialize), while
/// CI's tempfile SQLite exercises the true concurrent-writer contention the
/// WAL + busy_timeout configuration exists for.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_metering_inserts_never_fail_busy() {
    for test_db in test_dbs().await {
        let pool = test_db.pool.clone();
        let (_, key_id) = seed_key(&pool, None, "active", None, Some("concurrency-test")).await;

        const WRITERS: usize = 8;
        const ROWS_EACH: usize = 5;

        let mut handles = Vec::new();
        for writer in 0..WRITERS {
            let pool = pool.clone();
            let key_id = key_id.clone();
            handles.push(tokio::spawn(async move {
                for row in 0..ROWS_EACH {
                    let event = db::UsageEvent {
                        id: db::new_id(),
                        ts: db::now_ms(),
                        pool_key_id: key_id.clone(),
                        user_id: Some(format!("writer-{writer}")),
                        pool_id: None,
                        provider_key_id: None,
                        model: format!("m-{writer}-{row}"),
                        prompt_tokens: 1,
                        completion_tokens: 1,
                        cost_micros: 0,
                        latency_ms: 1,
                        status: "200".to_string(),
                    };
                    // The assertion itself: no writer may error. A missing
                    // busy_timeout surfaces here as `database is locked`.
                    db::insert_usage_event(&pool, &event)
                        .await
                        .expect("insert must queue, not fail (SQLITE_BUSY regression)");
                }
            }));
        }
        for handle in handles {
            handle.await.expect("writer task panicked");
        }

        let rows = db::usage_events_for_key(&pool, &key_id)
            .await
            .expect("read back");
        assert_eq!(
            rows.len(),
            WRITERS * ROWS_EACH,
            "every metered request must land exactly once (db: {})",
            test_db.name
        );
    }
}
