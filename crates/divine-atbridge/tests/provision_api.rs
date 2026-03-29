use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use diesel::Connection;
use diesel::PgConnection;
use diesel::QueryableByName;
use diesel::RunQueryDsl;
use diesel::sql_types::{Binary, Text};
use divine_atbridge::config::BridgeConfig;
use divine_atbridge::health::app_with_config;
use divine_bridge_db::{get_account_link_lifecycle, upsert_pending_account_link};
use serde_json::{json, Value};
use tower::util::ServiceExt;

const AUTH_HEADER: &str = "Bearer test-provisioning-token";
const TEST_KEY_HEX: &str =
    "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

#[derive(Debug, QueryableByName)]
struct StoredProvisioningKey {
    #[diesel(sql_type = Text)]
    key_ref: String,
    #[diesel(sql_type = Text)]
    key_purpose: String,
    #[diesel(sql_type = Text)]
    public_key_hex: String,
    #[diesel(sql_type = Binary)]
    encrypted_secret: Vec<u8>,
}

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
        include_str!("../../../migrations/004_provisioning_keys/down.sql"),
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
        include_str!("../../../migrations/004_provisioning_keys/up.sql"),
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
        .match_header("authorization", "Basic YWRtaW46YWRtaW4tdG9rZW4=")
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
        provisioning_key_encryption_key_hex: TEST_KEY_HEX.into(),
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

    let persisted_keys = diesel::sql_query(
        "SELECT key_ref, key_purpose, public_key_hex, encrypted_secret
         FROM provisioning_keys
         ORDER BY key_purpose ASC, key_ref ASC",
    )
    .load::<StoredProvisioningKey>(&mut conn)
    .expect("provisioning keys should load");
    assert_eq!(persisted_keys.len(), 2, "provisioning should persist signing and rotation keys");
    assert_eq!(
        persisted_keys
            .iter()
            .map(|row| row.key_purpose.as_str())
            .collect::<Vec<_>>(),
        vec!["plc-rotation-key", "signing-key"]
    );
    for row in persisted_keys {
        assert!(
            !row.key_ref.is_empty(),
            "persisted provisioning key ref should not be empty"
        );
        assert_eq!(
            row.public_key_hex.len(),
            66,
            "compressed secp256k1 public keys should be stored as 33-byte hex"
        );
        assert!(
            row.encrypted_secret.len() > 32,
            "encrypted secret should include nonce and authentication tag"
        );
    }
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
        provisioning_key_encryption_key_hex: TEST_KEY_HEX.into(),
    });

    assert!(
        result.is_err(),
        "configured app should fail closed without a provisioning token"
    );
}
