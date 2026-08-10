use axum::body::Body;
use axum::http::{Request, StatusCode};
use diesel::prelude::*;
use diesel::Connection;
use diesel::PgConnection;
use diesel::RunQueryDsl;
use divine_bridge_db::schema::labeler_events;
use divine_labeler::config::LabelerConfig;
use divine_labeler::routes::webhook::WebhookPayload;
use divine_labeler::{app_with_state, AppState};
use serde_json::json;
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
        include_str!("../../../migrations/001_bridge_tables/down.sql"),
    );
    execute_batch(
        &mut conn,
        include_str!("../../../migrations/001_bridge_tables/up.sql"),
    );
    execute_batch(
        &mut conn,
        include_str!("../../../migrations/002_label_tracking/up.sql"),
    );
}

fn seed_record_mapping(database_url: &str) {
    let mut conn =
        PgConnection::establish(database_url).expect("test database should be reachable");
    execute_batch(
        &mut conn,
        "INSERT INTO account_links (
            nostr_pubkey, did, handle, crosspost_enabled, signing_key_id,
            plc_rotation_key_ref, provisioning_state, provisioning_error, disabled_at
         ) VALUES (
            'npub1webhook',
            'did:plc:webhook',
            'webhook.test',
            true,
            'signing-key',
            'rotation-key',
            'ready',
            NULL,
            NULL
         );
         INSERT INTO record_mappings (
            nostr_event_id, did, collection, rkey, at_uri, cid, status
         ) VALUES (
            'nostr-webhook-event',
            'did:plc:webhook',
            'app.bsky.feed.post',
            'webhook-post',
            'at://did:plc:webhook/app.bsky.feed.post/webhook-post',
            'bafy-webhook-cid',
            'published'
         )",
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
fn webhook_payload_deserializes_from_js_format() {
    let json = r#"{
        "sha256": "abc123",
        "action": "QUARANTINE",
        "labels": [
            {"category": "nudity", "score": 0.91}
        ],
        "reviewed_by": null,
        "timestamp": "2026-03-20T12:00:00.000Z",
        "nostr_event_id": null
    }"#;

    let payload: WebhookPayload = serde_json::from_str(json).unwrap();
    assert_eq!(payload.sha256, "abc123");
    assert_eq!(payload.action, "QUARANTINE");
    assert_eq!(payload.labels.len(), 1);
    assert_eq!(payload.labels[0].category, "nudity");
}

#[test]
fn webhook_payload_handles_multiple_labels() {
    let json = r#"{
        "sha256": "def456",
        "action": "PERMANENT_BAN",
        "labels": [
            {"category": "violence", "score": 0.95},
            {"category": "gore", "score": 0.88}
        ],
        "reviewed_by": "admin",
        "timestamp": "2026-03-20T12:00:00.000Z",
        "nostr_event_id": "abc123eventid"
    }"#;

    let payload: WebhookPayload = serde_json::from_str(json).unwrap();
    assert_eq!(payload.labels.len(), 2);
    assert_eq!(payload.reviewed_by, Some("admin".to_string()));
    assert_eq!(payload.nostr_event_id, Some("abc123eventid".to_string()));
}

#[test]
fn webhook_payload_handles_empty_labels() {
    let json = r#"{
        "sha256": "ghi789",
        "action": "REVIEW",
        "labels": [],
        "reviewed_by": null,
        "timestamp": "2026-03-20T12:00:00.000Z",
        "nostr_event_id": null
    }"#;

    let payload: WebhookPayload = serde_json::from_str(json).unwrap();
    assert!(payload.labels.is_empty());
}

#[tokio::test]
#[serial]
async fn webhook_route_persists_labels_through_pooled_store() {
    let database_url = test_database_url();
    reset_database(&database_url);
    seed_record_mapping(&database_url);

    let app = build_app(database_url.clone());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook/moderation-result")
                .header("authorization", "Bearer test-webhook-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "sha256": "webhook-sha",
                        "action": "PERMANENT_BAN",
                        "labels": [{"category": "nudity", "score": 0.91}],
                        "reviewed_by": null,
                        "timestamp": "2026-08-09T12:00:00.000Z",
                        "nostr_event_id": "nostr-webhook-event"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["accepted"], 2);
    assert!(json["errors"].as_array().unwrap().is_empty());

    let mut conn =
        PgConnection::establish(&database_url).expect("test database should be reachable");
    let rows: Vec<(String, String, Option<String>)> = labeler_events::table
        .select((
            labeler_events::val,
            labeler_events::subject_uri,
            labeler_events::sha256,
        ))
        .order(labeler_events::seq.asc())
        .load(&mut conn)
        .expect("persisted labels should load");
    assert_eq!(
        rows,
        vec![
            (
                "nudity".to_string(),
                "at://did:plc:webhook/app.bsky.feed.post/webhook-post".to_string(),
                Some("webhook-sha".to_string())
            ),
            (
                "!takedown".to_string(),
                "at://did:plc:webhook/app.bsky.feed.post/webhook-post".to_string(),
                Some("webhook-sha".to_string())
            )
        ]
    );
    drop(app);
    settle_pool_teardown().await;
}

#[tokio::test]
#[serial]
async fn webhook_route_falls_back_to_sha_uri_through_pooled_store() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let app = build_app(database_url.clone());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook/moderation-result")
                .header("authorization", "Bearer test-webhook-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "sha256": "fallback-sha",
                        "action": "QUARANTINE",
                        "labels": [{"category": "violence", "score": 0.95}],
                        "reviewed_by": "admin",
                        "timestamp": "2026-08-09T12:00:00.000Z",
                        "nostr_event_id": "missing-event"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let mut conn =
        PgConnection::establish(&database_url).expect("test database should be reachable");
    let rows: Vec<(String, String, String)> = labeler_events::table
        .select((
            labeler_events::val,
            labeler_events::subject_uri,
            labeler_events::origin,
        ))
        .order(labeler_events::seq.asc())
        .load(&mut conn)
        .expect("persisted labels should load");
    assert_eq!(
        rows,
        vec![(
            "violence".to_string(),
            "at://sha256:fallback-sha".to_string(),
            "human".to_string()
        )]
    );
    drop(app);
    settle_pool_teardown().await;
}
