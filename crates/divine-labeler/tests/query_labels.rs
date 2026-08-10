use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Utc;
use diesel::Connection;
use diesel::PgConnection;
use diesel::RunQueryDsl;
use divine_bridge_db::models::LabelerEvent;
use divine_bridge_db::models::NewLabelerEvent;
use divine_labeler::config::LabelerConfig;
use divine_labeler::routes::query_labels::build_query_response;
use divine_labeler::store::DbStore;
use divine_labeler::{app_with_state, AppState};
use serial_test::serial;
use tower::util::ServiceExt;

fn test_database_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://divine:divine_dev@[::1]:5432/divine_bridge".to_string())
}

fn execute_batch(conn: &mut PgConnection, sql: &str) {
    for statement in sql
        .split(';')
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        diesel::sql_query(statement).execute(conn).unwrap();
    }
}

fn reset_database(database_url: &str) {
    let mut conn =
        PgConnection::establish(database_url).expect("test database should be reachable");
    execute_batch(
        &mut conn,
        include_str!("../../../migrations/002_label_tracking/down.sql"),
    );
    execute_batch(
        &mut conn,
        include_str!("../../../migrations/002_label_tracking/up.sql"),
    );
}

fn build_app(database_url: String) -> axum::Router {
    let state = AppState::from_config(LabelerConfig {
        labeler_did: "did:plc:test-labeler".to_string(),
        signing_key_hex: "11".repeat(32),
        database_url,
        webhook_token: "test-webhook-token".to_string(),
        port: 3001,
    })
    .expect("labeler app state should build");
    app_with_state(state)
}

// r2d2 replenishes toward `min_idle` in the background, so the last checkout can
// leave a `PQconnectdb` running on the pool's thread pool. That handshake is
// inside OpenSSL, and libc runs `OPENSSL_cleanup` as soon as the test binary
// reaches `exit()`; the two race and segfault the process after the assertions
// have already passed.
//
// Call this once nothing in the test still holds a pool. Dropping first is what
// stops r2d2 scheduling more connects, because a queued `add_connection` job
// only holds a `Weak` to the pool; this wait is for the one that already
// started.
async fn settle_pool_teardown() {
    // An in-flight connect was measured to finish well inside 25ms even with
    // every core saturated. Keep an order of magnitude of headroom for CI.
    const SETTLE: std::time::Duration = std::time::Duration::from_millis(250);
    tokio::time::sleep(SETTLE).await;
}

#[test]
fn build_query_response_formats_labels_correctly() {
    let events = vec![LabelerEvent {
        seq: 1,
        src_did: "did:plc:test-labeler".to_string(),
        subject_uri: "at://did:plc:user1/app.bsky.feed.post/rkey1".to_string(),
        subject_cid: None,
        val: "nudity".to_string(),
        neg: false,
        nostr_event_id: None,
        sha256: Some("abc123".to_string()),
        origin: "divine".to_string(),
        created_at: Utc::now(),
    }];

    let (body, cursor) = build_query_response(&events);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(json["labels"].is_array());
    assert_eq!(json["labels"][0]["val"], "nudity");
    assert_eq!(json["labels"][0]["src"], "did:plc:test-labeler");
    assert_eq!(json["labels"][0]["ver"], 1);
    assert!(cursor.is_some());
}

#[test]
fn build_query_response_empty_events_returns_empty_labels() {
    let (body, cursor) = build_query_response(&[]);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["labels"].as_array().unwrap().len(), 0);
    assert!(cursor.is_none());
}

#[tokio::test]
#[serial]
async fn query_labels_route_reads_events_through_pooled_store() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let store = DbStore::connect(&database_url).expect("store should connect");
    let first = store
        .insert_labeler_event(&NewLabelerEvent {
            src_did: "did:plc:test-labeler",
            subject_uri: "at://did:plc:user/app.bsky.feed.post/first",
            subject_cid: Some("bafy-first"),
            val: "nudity",
            neg: false,
            nostr_event_id: Some("nostr-first"),
            sha256: Some("sha-first"),
            origin: "divine",
        })
        .await
        .expect("first label should insert");
    store
        .insert_labeler_event(&NewLabelerEvent {
            src_did: "did:plc:test-labeler",
            subject_uri: "at://did:plc:user/app.bsky.feed.post/second",
            subject_cid: None,
            val: "violence",
            neg: false,
            nostr_event_id: Some("nostr-second"),
            sha256: Some("sha-second"),
            origin: "human",
        })
        .await
        .expect("second label should insert");

    let app = build_app(database_url);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/xrpc/com.atproto.label.queryLabels?cursor={}&limit=10&uriPatterns=at://did:plc:user/app.bsky.feed.post/*",
                    first.seq
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["labels"].as_array().unwrap().len(), 1);
    assert_eq!(json["labels"][0]["val"], "violence");
    assert_eq!(
        json["labels"][0]["uri"],
        "at://did:plc:user/app.bsky.feed.post/second"
    );
    assert_eq!(json["cursor"], (first.seq + 1).to_string());
    drop((app, store));
    settle_pool_teardown().await;
}
