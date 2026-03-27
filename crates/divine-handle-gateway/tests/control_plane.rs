use axum::body::to_bytes;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use diesel::Connection;
use diesel::PgConnection;
use diesel::RunQueryDsl;
use divine_handle_gateway::{app_with_config, AppConfig};
use mockito::Matcher;
use serde_json::json;
use serde_json::Value;
use serial_test::serial;
use tower::util::ServiceExt;

const AUTH_HEADER: &str = "Bearer test-keycast-token";

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body to be readable");
    serde_json::from_slice(&bytes).expect("response to contain valid JSON")
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
        "DROP SCHEMA IF EXISTS public CASCADE;
         CREATE SCHEMA public;",
    );
    execute_batch(&mut conn, include_str!("../../../migrations/001_bridge_tables/up.sql"));
}

fn build_app(database_url: String, name_server_url: String) -> axum::Router {
    let config = AppConfig {
        database_url,
        keycast_atproto_token: "test-keycast-token".to_string(),
        atproto_provisioning_url: format!("{name_server_url}/provision"),
        atproto_provisioning_token: None,
        atproto_keycast_sync_url: format!("{name_server_url}/api/internal/atproto/state"),
        atproto_name_server_sync_url: format!(
            "{name_server_url}/api/internal/username/set-atproto"
        ),
        atproto_name_server_sync_token: "test-sync-token".to_string(),
    };
    app_with_config(config).expect("test app should build")
}

#[tokio::test]
#[serial]
async fn control_plane_health_endpoints_are_public() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let mut name_server = mockito::Server::new_async().await;
    let _sync_stub = name_server
        .mock("POST", "/api/internal/username/set-atproto")
        .with_status(200)
        .create_async()
        .await;
    let _keycast_sync_stub = name_server
        .mock("POST", "/api/internal/atproto/state")
        .with_status(200)
        .create_async()
        .await;

    let app = build_app(database_url, name_server.url());

    for path in ["/health", "/health/ready"] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "path {path}");
    }
}

#[tokio::test]
#[serial]
async fn control_plane_opt_in_creates_pending_status() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let mut name_server = mockito::Server::new_async().await;
    let _sync_stub = name_server
        .mock("POST", "/api/internal/username/set-atproto")
        .with_status(200)
        .create_async()
        .await;
    let _keycast_sync_stub = name_server
        .mock("POST", "/api/internal/atproto/state")
        .with_status(200)
        .create_async()
        .await;

    let app = build_app(database_url, name_server.url());

    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account-links/opt-in")
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
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account-links/opt-in")
                .header("content-type", "application/json")
                .header("authorization", AUTH_HEADER)
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

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let payload = response_json(response).await;
    assert_eq!(payload["provisioning_state"], "pending");
    assert_eq!(payload["crosspost_enabled"], true);
}

#[tokio::test]
#[serial]
async fn control_plane_status_and_export_reflect_provisioned_link() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let mut name_server = mockito::Server::new_async().await;
    let _sync_stub = name_server
        .mock("POST", "/api/internal/username/set-atproto")
        .with_status(200)
        .create_async()
        .await;
    let _keycast_sync_stub = name_server
        .mock("POST", "/api/internal/atproto/state")
        .with_status(200)
        .create_async()
        .await;

    let app = build_app(database_url, name_server.url());

    let provision_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account-links/provision")
                .header("content-type", "application/json")
                .header("authorization", AUTH_HEADER)
                .body(Body::from(
                    json!({
                        "nostr_pubkey": "npub1alice",
                        "handle": "alice.divine.video",
                        "did": "did:plc:alice123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(provision_response.status(), StatusCode::OK);

    let status_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/account-links/npub1alice/status")
                .header("authorization", AUTH_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(status_response.status(), StatusCode::OK);

    let export_response = app
        .oneshot(
            Request::builder()
                .uri("/api/account-links/npub1alice/export")
                .header("authorization", AUTH_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(export_response.status(), StatusCode::OK);
}

#[tokio::test]
#[serial]
async fn control_plane_manual_provision_syncs_ready_state_downstream() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let mut name_server = mockito::Server::new_async().await;
    let keycast_sync = name_server
        .mock("POST", "/api/internal/atproto/state")
        .match_header("authorization", AUTH_HEADER)
        .match_body(Matcher::Json(json!({
            "nostr_pubkey": "npub1alice",
            "enabled": true,
            "state": "ready",
            "did": "did:plc:alice123",
            "error": Value::Null
        })))
        .with_status(200)
        .create_async()
        .await;
    let name_server_sync = name_server
        .mock("POST", "/api/internal/username/set-atproto")
        .match_header("authorization", "Bearer test-sync-token")
        .match_body(Matcher::Json(json!({
            "name": "alice",
            "atproto_did": "did:plc:alice123",
            "atproto_state": "ready"
        })))
        .with_status(200)
        .create_async()
        .await;

    let app = build_app(database_url, name_server.url());

    let provision_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account-links/provision")
                .header("content-type", "application/json")
                .header("authorization", AUTH_HEADER)
                .body(Body::from(
                    json!({
                        "nostr_pubkey": "npub1alice",
                        "handle": "alice.divine.video",
                        "did": "did:plc:alice123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(provision_response.status(), StatusCode::OK);
    keycast_sync.assert_async().await;
    name_server_sync.assert_async().await;
}

#[tokio::test]
#[serial]
async fn control_plane_export_returns_internal_error_on_store_failure() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let mut name_server = mockito::Server::new_async().await;
    let _sync_stub = name_server
        .mock("POST", "/api/internal/username/set-atproto")
        .with_status(200)
        .create_async()
        .await;
    let _keycast_sync_stub = name_server
        .mock("POST", "/api/internal/atproto/state")
        .with_status(200)
        .create_async()
        .await;

    let app = build_app(database_url.clone(), name_server.url());

    {
        let mut conn =
            PgConnection::establish(&database_url).expect("test database should be reachable");
        execute_batch(&mut conn, "drop table account_links cascade");
    }

    let export_response = app
        .oneshot(
            Request::builder()
                .uri("/api/account-links/npub1alice/export")
                .header("authorization", AUTH_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(export_response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
#[serial]
async fn control_plane_disable_does_not_expose_public_well_known_resolution() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let mut name_server = mockito::Server::new_async().await;
    let _sync_stub = name_server
        .mock("POST", "/api/internal/username/set-atproto")
        .with_status(200)
        .create_async()
        .await;
    let _keycast_sync_stub = name_server
        .mock("POST", "/api/internal/atproto/state")
        .with_status(200)
        .create_async()
        .await;

    let app = build_app(database_url, name_server.url());

    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account-links/provision")
                .header("content-type", "application/json")
                .header("authorization", AUTH_HEADER)
                .body(Body::from(
                    json!({
                        "nostr_pubkey": "npub1bob",
                        "handle": "bob.divine.video",
                        "did": "did:plc:bob123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let well_known_ready = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/.well-known/atproto-did")
                .header("host", "bob.divine.video")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(well_known_ready.status(), StatusCode::NOT_FOUND);

    let disable_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account-links/npub1bob/disable")
                .header("authorization", AUTH_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(disable_response.status(), StatusCode::OK);
    let payload = response_json(disable_response).await;
    assert_eq!(payload["provisioning_state"], "disabled");
    assert_eq!(payload["crosspost_enabled"], false);

    let well_known_disabled = app
        .oneshot(
            Request::builder()
                .uri("/.well-known/atproto-did")
                .header("host", "bob.divine.video")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(well_known_disabled.status(), StatusCode::NOT_FOUND);
}
