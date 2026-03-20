use axum::body::Body;
use axum::http::{Request, StatusCode};
use divine_handle_gateway::app;
use serde_json::json;
use tower::util::ServiceExt;

#[tokio::test]
async fn control_plane_opt_in_creates_pending_status() {
    let app = app();

    let response = app
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

    assert_eq!(response.status(), StatusCode::ACCEPTED);
}

#[tokio::test]
async fn control_plane_status_and_export_reflect_provisioned_link() {
    let app = app();

    let provision_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account-links/provision")
                .header("content-type", "application/json")
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
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(export_response.status(), StatusCode::OK);
}

#[tokio::test]
async fn control_plane_disable_blocks_host_resolution() {
    let app = app();

    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account-links/provision")
                .header("content-type", "application/json")
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

    assert_eq!(well_known_ready.status(), StatusCode::OK);

    let disable_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account-links/npub1bob/disable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(disable_response.status(), StatusCode::OK);

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
