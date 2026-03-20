use axum::body::Body;
use axum::http::{Request, StatusCode};
use divine_labeler::config::LabelerConfig;
use divine_labeler::{app_with_state, AppState};
use tower::util::ServiceExt;

fn test_database_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://divine:divine_dev@[::1]:5432/divine_bridge".to_string())
}

fn build_app() -> axum::Router {
    let config = LabelerConfig {
        labeler_did: "did:plc:test-labeler".to_string(),
        signing_key_hex: "11".repeat(32),
        database_url: test_database_url(),
        webhook_token: "test-webhook-token".to_string(),
        port: 3001,
    };

    let state = AppState::from_config(config).expect("labeler app state should build");
    app_with_state(state)
}

#[tokio::test]
async fn labeler_health_endpoints_return_ok() {
    let app = build_app();

    for path in ["/health", "/health/ready"] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK, "path {path}");
    }
}
