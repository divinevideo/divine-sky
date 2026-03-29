use std::time::Duration;

use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use diesel::Connection;
use diesel::PgConnection;
use diesel::RunQueryDsl;
use divine_handle_gateway::keycast_client::KeycastClient;
use divine_handle_gateway::name_server_client::NameServerClient;
use divine_handle_gateway::provision_runner::{ProvisionRunner, ProvisioningClient};
use divine_handle_gateway::store::DbStore;
use divine_handle_gateway::{app_with_config, AppConfig};
use mockito::Matcher;
use serde_json::{json, Value};
use serial_test::serial;
use tower::util::ServiceExt;

const AUTH_HEADER: &str = "Bearer test-keycast-token";

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
    execute_batch(&mut conn, include_str!("../../../migrations/001_bridge_tables/up.sql"));
    execute_batch(
        &mut conn,
        include_str!("../../../migrations/004_publish_job_scheduler/up.sql"),
    );
}

fn insert_account_link_row(database_url: &str, values_sql: &str) {
    let mut conn =
        PgConnection::establish(database_url).expect("test database should be reachable");
    diesel::sql_query(format!(
        "INSERT INTO account_links (
            nostr_pubkey, did, handle, crosspost_enabled, signing_key_id,
            plc_rotation_key_ref, provisioning_state, provisioning_error,
            disabled_at, created_at, updated_at
         ) VALUES {values_sql}"
    ))
    .execute(&mut conn)
    .expect("account link row should insert");
}

fn build_runner(
    database_url: &str,
    provision_url: String,
    name_server_url: String,
    keycast_url: String,
) -> ProvisionRunner {
    ProvisionRunner::new(
        DbStore::connect(database_url).expect("store should connect"),
        ProvisioningClient::new(provision_url, None),
        NameServerClient::new(name_server_url, "test-sync-token".to_string()),
        KeycastClient::new(keycast_url, "test-keycast-token".to_string()),
    )
}

fn build_app(
    database_url: String,
    provision_url: String,
    name_server_url: String,
    keycast_url: String,
) -> axum::Router {
    let config = AppConfig {
        database_url,
        keycast_atproto_token: "test-keycast-token".to_string(),
        atproto_provisioning_url: provision_url,
        atproto_provisioning_token: None,
        atproto_keycast_sync_url: keycast_url,
        atproto_name_server_sync_url: name_server_url,
        atproto_name_server_sync_token: "test-sync-token".to_string(),
    };
    app_with_config(config).expect("test app should build")
}

async fn post_json(
    app: axum::Router,
    uri: &str,
    auth: Option<&str>,
    body: Value,
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(token) = auth {
        builder = builder.header("authorization", token);
    }
    app.oneshot(
        builder
            .body(axum::body::Body::from(body.to_string()))
            .unwrap(),
    )
    .await
    .unwrap()
}

async fn get_json(app: axum::Router, uri: &str, auth: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", auth)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload = serde_json::from_slice(&bytes).unwrap_or_else(|_| json!({}));
    (status, payload)
}

#[tokio::test]
#[serial]
async fn successful_provision_syncs_ready_state_to_name_server() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let mut provision_server = mockito::Server::new_async().await;
    let mut name_server = mockito::Server::new_async().await;
    let mut keycast = mockito::Server::new_async().await;

    let provision_mock = provision_server
        .mock("POST", "/provision")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "did": "did:plc:abc",
                "handle": "alice.divine.video",
                "signing_key_id": "k1"
            })
            .to_string(),
        )
        .create_async()
        .await;

    let sync_ready_mock = name_server
        .mock("POST", "/api/internal/username/set-atproto")
        .match_header("authorization", "Bearer test-sync-token")
        .match_body(Matcher::Json(json!({
            "name": "alice",
            "atproto_did": "did:plc:abc",
            "atproto_state": "ready"
        })))
        .with_status(200)
        .create_async()
        .await;

    let keycast_ready_mock = keycast
        .mock("POST", "/api/internal/atproto/state")
        .match_header("authorization", "Bearer test-keycast-token")
        .match_body(Matcher::Json(json!({
            "nostr_pubkey": "npub1alice",
            "enabled": true,
            "state": "ready",
            "did": "did:plc:abc",
            "error": Value::Null
        })))
        .with_status(200)
        .create_async()
        .await;

    let app = build_app(
        database_url,
        format!("{}/provision", provision_server.url()),
        format!("{}/api/internal/username/set-atproto", name_server.url()),
        format!("{}/api/internal/atproto/state", keycast.url()),
    );
    let response = post_json(
        app.clone(),
        "/api/account-links/opt-in",
        Some(AUTH_HEADER),
        json!({
            "nostr_pubkey": "npub1alice",
            "handle": "alice.divine.video"
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let mut ready_payload = None;
    for _ in 0..20 {
        let (status, payload) = get_json(
            app.clone(),
            "/api/account-links/npub1alice/status",
            AUTH_HEADER,
        )
        .await;
        if status == StatusCode::OK && payload["provisioning_state"] == "ready" {
            ready_payload = Some(payload);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let payload = ready_payload.expect("status should transition to ready");
    assert_eq!(payload["did"], "did:plc:abc");

    provision_mock.assert_async().await;
    keycast_ready_mock.assert_async().await;
    sync_ready_mock.assert_async().await;
}

#[tokio::test]
#[serial]
async fn failed_provision_syncs_failed_state_to_keycast_and_name_server() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let mut provision_server = mockito::Server::new_async().await;
    let mut name_server = mockito::Server::new_async().await;
    let mut keycast = mockito::Server::new_async().await;

    let provision_mock = provision_server
        .mock("POST", "/provision")
        .with_status(502)
        .with_body("upstream unavailable")
        .create_async()
        .await;

    let keycast_failed_mock = keycast
        .mock("POST", "/api/internal/atproto/state")
        .match_header("authorization", "Bearer test-keycast-token")
        .match_body(Matcher::PartialJson(json!({
            "nostr_pubkey": "npub1alice",
            "enabled": true,
            "state": "failed",
            "did": Value::Null
        })))
        .with_status(200)
        .create_async()
        .await;

    let sync_failed_mock = name_server
        .mock("POST", "/api/internal/username/set-atproto")
        .match_header("authorization", "Bearer test-sync-token")
        .match_body(Matcher::Json(json!({
            "name": "alice",
            "atproto_did": Value::Null,
            "atproto_state": "failed"
        })))
        .with_status(200)
        .create_async()
        .await;

    let app = build_app(
        database_url,
        format!("{}/provision", provision_server.url()),
        format!("{}/api/internal/username/set-atproto", name_server.url()),
        format!("{}/api/internal/atproto/state", keycast.url()),
    );
    let response = post_json(
        app.clone(),
        "/api/account-links/opt-in",
        Some(AUTH_HEADER),
        json!({
            "nostr_pubkey": "npub1alice",
            "handle": "alice.divine.video"
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let mut failed_payload = None;
    for _ in 0..20 {
        let (status, payload) = get_json(
            app.clone(),
            "/api/account-links/npub1alice/status",
            AUTH_HEADER,
        )
        .await;
        if status == StatusCode::OK && payload["provisioning_state"] == "failed" {
            failed_payload = Some(payload);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let payload = failed_payload.expect("status should transition to failed");
    assert_eq!(payload["did"], Value::Null);
    assert!(payload["provisioning_error"]
        .as_str()
        .expect("failed status to carry an error")
        .contains("provisioning request returned non-success status"));

    provision_mock.assert_async().await;
    keycast_failed_mock.assert_async().await;
    sync_failed_mock.assert_async().await;
}

#[tokio::test]
#[serial]
async fn startup_replay_retries_preexisting_pending_rows() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let mut conn =
        PgConnection::establish(&database_url).expect("test database should be reachable");
    diesel::sql_query(
        "INSERT INTO account_links (
            nostr_pubkey, did, handle, crosspost_enabled, signing_key_id,
            plc_rotation_key_ref, provisioning_state, provisioning_error,
            disabled_at, created_at, updated_at
         ) VALUES (
            'npub1replay', NULL, 'replay.divine.video', TRUE, 'pending-signing:npub1replay',
            'pending-rotation:npub1replay', 'pending', NULL, NULL, NOW(), NOW()
         )",
    )
    .execute(&mut conn)
    .expect("pending row should insert");

    let mut provision_server = mockito::Server::new_async().await;
    let mut name_server = mockito::Server::new_async().await;
    let mut keycast = mockito::Server::new_async().await;

    let provision_mock = provision_server
        .mock("POST", "/provision")
        .match_body(Matcher::Json(json!({
            "nostr_pubkey": "npub1replay",
            "handle": "replay.divine.video"
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "did": "did:plc:replay",
                "handle": "replay.divine.video",
                "signing_key_id": "k-replay"
            })
            .to_string(),
        )
        .create_async()
        .await;

    let keycast_ready_mock = keycast
        .mock("POST", "/api/internal/atproto/state")
        .match_header("authorization", "Bearer test-keycast-token")
        .match_body(Matcher::Json(json!({
            "nostr_pubkey": "npub1replay",
            "enabled": true,
            "state": "ready",
            "did": "did:plc:replay",
            "error": Value::Null
        })))
        .with_status(200)
        .create_async()
        .await;

    let name_server_ready_mock = name_server
        .mock("POST", "/api/internal/username/set-atproto")
        .match_header("authorization", "Bearer test-sync-token")
        .match_body(Matcher::Json(json!({
            "name": "replay",
            "atproto_did": "did:plc:replay",
            "atproto_state": "ready"
        })))
        .with_status(200)
        .create_async()
        .await;

    let runner = ProvisionRunner::new(
        DbStore::connect(&database_url).expect("store should connect"),
        ProvisioningClient::new(format!("{}/provision", provision_server.url()), None),
        NameServerClient::new(
            format!("{}/api/internal/username/set-atproto", name_server.url()),
            "test-sync-token".to_string(),
        ),
        KeycastClient::new(
            format!("{}/api/internal/atproto/state", keycast.url()),
            "test-keycast-token".to_string(),
        ),
    );

    let replayed = runner
        .replay_pending_from_database(&database_url)
        .await
        .expect("startup replay should succeed");
    assert_eq!(replayed, 1);

    provision_mock.assert_async().await;
    keycast_ready_mock.assert_async().await;
    name_server_ready_mock.assert_async().await;

    let app = build_app(
        database_url,
        format!("{}/provision", provision_server.url()),
        format!("{}/api/internal/username/set-atproto", name_server.url()),
        format!("{}/api/internal/atproto/state", keycast.url()),
    );
    let (status, payload) =
        get_json(app, "/api/account-links/npub1replay/status", AUTH_HEADER).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["provisioning_state"], "ready");
    assert_eq!(payload["did"], "did:plc:replay");
}

#[tokio::test]
#[serial]
async fn startup_reconciliation_republishes_ready_rows() {
    let database_url = test_database_url();
    reset_database(&database_url);
    insert_account_link_row(
        &database_url,
        "(
            'npub1ready', 'did:plc:ready1', 'ready.divine.video', TRUE, 'signing-key:ready',
            'rotation-key:ready', 'ready', NULL, NULL, NOW(), NOW()
        )",
    );

    let provision_server = mockito::Server::new_async().await;
    let mut name_server = mockito::Server::new_async().await;
    let mut keycast = mockito::Server::new_async().await;

    let keycast_ready_mock = keycast
        .mock("POST", "/api/internal/atproto/state")
        .match_header("authorization", "Bearer test-keycast-token")
        .match_body(Matcher::Json(json!({
            "nostr_pubkey": "npub1ready",
            "enabled": true,
            "state": "ready",
            "did": "did:plc:ready1",
            "error": Value::Null
        })))
        .with_status(200)
        .create_async()
        .await;

    let name_server_ready_mock = name_server
        .mock("POST", "/api/internal/username/set-atproto")
        .match_header("authorization", "Bearer test-sync-token")
        .match_body(Matcher::Json(json!({
            "name": "ready",
            "atproto_did": "did:plc:ready1",
            "atproto_state": "ready"
        })))
        .with_status(200)
        .create_async()
        .await;

    let runner = build_runner(
        &database_url,
        format!("{}/provision", provision_server.url()),
        format!("{}/api/internal/username/set-atproto", name_server.url()),
        format!("{}/api/internal/atproto/state", keycast.url()),
    );

    let reconciled = runner
        .reconcile_existing_from_database(&database_url)
        .await
        .expect("startup reconciliation should succeed");
    assert_eq!(reconciled, 1);

    keycast_ready_mock.assert_async().await;
    name_server_ready_mock.assert_async().await;
}

#[tokio::test]
#[serial]
async fn startup_reconciliation_republishes_failed_rows() {
    let database_url = test_database_url();
    reset_database(&database_url);
    insert_account_link_row(
        &database_url,
        "(
            'npub1failed', 'did:plc:failed1', 'failed.divine.video', TRUE, 'signing-key:failed',
            'rotation-key:failed', 'failed', 'createAccount failed', NULL, NOW(), NOW()
        )",
    );

    let provision_server = mockito::Server::new_async().await;
    let mut name_server = mockito::Server::new_async().await;
    let mut keycast = mockito::Server::new_async().await;

    let keycast_failed_mock = keycast
        .mock("POST", "/api/internal/atproto/state")
        .match_header("authorization", "Bearer test-keycast-token")
        .match_body(Matcher::Json(json!({
            "nostr_pubkey": "npub1failed",
            "enabled": true,
            "state": "failed",
            "did": Value::Null,
            "error": "createAccount failed"
        })))
        .with_status(200)
        .create_async()
        .await;

    let name_server_failed_mock = name_server
        .mock("POST", "/api/internal/username/set-atproto")
        .match_header("authorization", "Bearer test-sync-token")
        .match_body(Matcher::Json(json!({
            "name": "failed",
            "atproto_did": Value::Null,
            "atproto_state": "failed"
        })))
        .with_status(200)
        .create_async()
        .await;

    let runner = build_runner(
        &database_url,
        format!("{}/provision", provision_server.url()),
        format!("{}/api/internal/username/set-atproto", name_server.url()),
        format!("{}/api/internal/atproto/state", keycast.url()),
    );

    let reconciled = runner
        .reconcile_existing_from_database(&database_url)
        .await
        .expect("startup reconciliation should succeed");
    assert_eq!(reconciled, 1);

    keycast_failed_mock.assert_async().await;
    name_server_failed_mock.assert_async().await;
}

#[tokio::test]
#[serial]
async fn startup_reconciliation_republishes_disabled_rows() {
    let database_url = test_database_url();
    reset_database(&database_url);
    insert_account_link_row(
        &database_url,
        "(
            'npub1disabled', 'did:plc:disabled1', 'disabled.divine.video', FALSE, 'signing-key:disabled',
            'rotation-key:disabled', 'disabled', NULL, NOW(), NOW(), NOW()
        )",
    );

    let provision_server = mockito::Server::new_async().await;
    let mut name_server = mockito::Server::new_async().await;
    let mut keycast = mockito::Server::new_async().await;

    let keycast_disabled_mock = keycast
        .mock("POST", "/api/internal/atproto/state")
        .match_header("authorization", "Bearer test-keycast-token")
        .match_body(Matcher::Json(json!({
            "nostr_pubkey": "npub1disabled",
            "enabled": false,
            "state": "disabled",
            "did": Value::Null,
            "error": Value::Null
        })))
        .with_status(200)
        .create_async()
        .await;

    let name_server_disabled_mock = name_server
        .mock("POST", "/api/internal/username/set-atproto")
        .match_header("authorization", "Bearer test-sync-token")
        .match_body(Matcher::Json(json!({
            "name": "disabled",
            "atproto_did": Value::Null,
            "atproto_state": "disabled"
        })))
        .with_status(200)
        .create_async()
        .await;

    let runner = build_runner(
        &database_url,
        format!("{}/provision", provision_server.url()),
        format!("{}/api/internal/username/set-atproto", name_server.url()),
        format!("{}/api/internal/atproto/state", keycast.url()),
    );

    let reconciled = runner
        .reconcile_existing_from_database(&database_url)
        .await
        .expect("startup reconciliation should succeed");
    assert_eq!(reconciled, 1);

    keycast_disabled_mock.assert_async().await;
    name_server_disabled_mock.assert_async().await;
}

#[tokio::test]
#[serial]
async fn disable_syncs_disabled_state_to_name_server() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let mut provision_server = mockito::Server::new_async().await;
    let _provision_stub = provision_server
        .mock("POST", "/provision")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "did": "did:plc:abc",
                "handle": "alice.divine.video",
                "signing_key_id": "k1"
            })
            .to_string(),
        )
        .create_async()
        .await;

    let mut name_server = mockito::Server::new_async().await;
    let mut keycast = mockito::Server::new_async().await;

    let sync_disabled_mock = name_server
        .mock("POST", "/api/internal/username/set-atproto")
        .match_header("authorization", "Bearer test-sync-token")
        .match_body(Matcher::Json(json!({
            "name": "alice",
            "atproto_did": Value::Null,
            "atproto_state": "disabled"
        })))
        .with_status(200)
        .create_async()
        .await;

    let keycast_disabled_mock = keycast
        .mock("POST", "/api/internal/atproto/state")
        .match_header("authorization", "Bearer test-keycast-token")
        .match_body(Matcher::Json(json!({
            "nostr_pubkey": "npub1alice",
            "enabled": false,
            "state": "disabled",
            "did": Value::Null,
            "error": Value::Null
        })))
        .with_status(200)
        .create_async()
        .await;

    let app = build_app(
        database_url,
        format!("{}/provision", provision_server.url()),
        format!("{}/api/internal/username/set-atproto", name_server.url()),
        format!("{}/api/internal/atproto/state", keycast.url()),
    );
    let _ = post_json(
        app.clone(),
        "/api/account-links/provision",
        Some(AUTH_HEADER),
        json!({
            "nostr_pubkey": "npub1alice",
            "handle": "alice.divine.video",
            "did": "did:plc:abc"
        }),
    )
    .await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account-links/npub1alice/disable")
                .header("authorization", AUTH_HEADER)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    keycast_disabled_mock.assert_async().await;
    sync_disabled_mock.assert_async().await;
}
