use axum::body::Body;
use axum::http::{Request, StatusCode};
use divine_atbridge::health::{app, app_with_runtime_state, RuntimeHealthState};
use tower::util::ServiceExt;

#[tokio::test]
async fn atbridge_health_endpoints_return_ok() {
    let app = app();

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
async fn atbridge_readiness_only_flips_after_sustained_runtime_failure() {
    let runtime = RuntimeHealthState::default();
    let app = app_with_runtime_state(runtime.clone());

    let initial = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(initial.status(), StatusCode::OK);

    runtime.record_relay_failure("temporary relay timeout");
    let transient = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(transient.status(), StatusCode::OK);

    runtime.record_relay_failure("relay still unavailable");
    runtime.record_relay_failure("relay still unavailable");
    let degraded = app
        .oneshot(
            Request::builder()
                .uri("/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(degraded.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn atbridge_readiness_ignores_processing_failures_from_bad_data() {
    let runtime = RuntimeHealthState::default();
    let app = app_with_runtime_state(runtime.clone());

    runtime.record_processing_failure("malformed relay frame");
    runtime.record_processing_failure("bad event payload");
    runtime.record_processing_failure("publisher rejected record");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn atbridge_readiness_recovers_after_a_later_success() {
    let runtime = RuntimeHealthState::default();
    let app = app_with_runtime_state(runtime.clone());

    runtime.record_relay_failure("relay unavailable");
    runtime.record_relay_failure("relay unavailable");
    runtime.record_relay_failure("relay unavailable");

    let degraded = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(degraded.status(), StatusCode::SERVICE_UNAVAILABLE);

    runtime.record_success();

    let recovered = app
        .oneshot(
            Request::builder()
                .uri("/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(recovered.status(), StatusCode::OK);
}

#[tokio::test]
async fn atbridge_provision_route_requires_auth() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/provision")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"nostr_pubkey":"npub1alice","handle":"alice.divine.video"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
