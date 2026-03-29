use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use diesel::Connection;
use diesel::PgConnection;
use diesel::RunQueryDsl;
use divine_atbridge::config::BridgeConfig;
use divine_atbridge::health::app_with_config;
use divine_bridge_db::{get_account_link_lifecycle, upsert_pending_account_link};
use serde_json::{json, Value};
use tower::util::ServiceExt;

const AUTH_HEADER: &str = "Bearer test-provisioning-token";

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
        include_str!("../../../migrations/001_bridge_tables/down.sql"),
    );
    execute_batch(
        &mut conn,
        include_str!("../../../migrations/001_bridge_tables/up.sql"),
    );
    execute_batch(
        &mut conn,
        include_str!("../../../migrations/004_publish_job_scheduler/up.sql"),
    );
}

#[tokio::test]
async fn configured_internal_api_provisions_pending_link() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let mut conn =
        PgConnection::establish(&database_url).expect("test database should be reachable");
    upsert_pending_account_link(
        &mut conn,
        "npub1alice",
        "alice.divine.video",
        "pending-signing:npub1alice",
        "pending-rotation:npub1alice",
        true,
    )
    .expect("pending row should seed");

    let mut plc_server = mockito::Server::new_async().await;
    let mut pds_server = mockito::Server::new_async().await;

    let plc_mock = plc_server
        .mock("POST", "/")
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(json!({ "did": "did:plc:alice123" }).to_string())
        .create_async()
        .await;

    let pds_mock = pds_server
        .mock("POST", "/xrpc/com.atproto.server.createAccount")
        .match_header("authorization", "Bearer admin-token")
        .with_status(200)
        .with_body("{}")
        .create_async()
        .await;

    let app = app_with_config(BridgeConfig {
        relay_url: "wss://relay.example.com".into(),
        pds_url: pds_server.url(),
        pds_auth_token: "admin-token".into(),
        blossom_url: "https://blossom.example.com".into(),
        database_url: database_url.clone(),
        s3_endpoint: "https://s3.example.com".into(),
        s3_bucket: "bucket".into(),
        relay_source_name: "nostr-relay".into(),
        health_bind_addr: "127.0.0.1:0".into(),
        plc_directory_url: plc_server.url(),
        handle_domain: "divine.video".into(),
        provisioning_bearer_token: "test-provisioning-token".into(),
        video_service_url: "https://video.bsky.app".into(),
        video_service_enabled: false,
        video_service_poll_timeout_secs: 120,
        video_service_poll_interval_ms: 5000,
    })
    .expect("configured app should build");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/provision")
                .header("authorization", AUTH_HEADER)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "nostr_pubkey": "npub1alice",
                        "handle": "alice.divine.video"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let payload: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
            .expect("provision route should return json");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["did"], "did:plc:alice123");
    assert_eq!(payload["handle"], "alice.divine.video");
    assert!(payload["signing_key_id"].as_str().unwrap_or_default().len() > 8);

    plc_mock.assert_async().await;
    pds_mock.assert_async().await;

    let stored = get_account_link_lifecycle(&mut conn, "npub1alice")
        .expect("row should load")
        .expect("row should exist");
    assert_eq!(stored.did.as_deref(), Some("did:plc:alice123"));
    assert_eq!(stored.provisioning_state, "ready");
}

#[test]
fn configured_internal_api_requires_provisioning_token() {
    let result = app_with_config(BridgeConfig {
        relay_url: "wss://relay.example.com".into(),
        pds_url: "https://pds.example.com".into(),
        pds_auth_token: "admin-token".into(),
        blossom_url: "https://blossom.example.com".into(),
        database_url: "postgres://divine:divine_dev@[::1]:5432/divine_bridge".into(),
        s3_endpoint: "https://s3.example.com".into(),
        s3_bucket: "bucket".into(),
        relay_source_name: "nostr-relay".into(),
        health_bind_addr: "127.0.0.1:0".into(),
        plc_directory_url: "https://plc.directory".into(),
        handle_domain: "divine.video".into(),
        provisioning_bearer_token: String::new(),
        video_service_url: "https://video.bsky.app".into(),
        video_service_enabled: false,
        video_service_poll_timeout_secs: 120,
        video_service_poll_interval_ms: 5000,
    });

    assert!(
        result.is_err(),
        "configured app should fail closed without a provisioning token"
    );
}
