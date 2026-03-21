use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::util::ServiceExt;

#[tokio::test]
async fn creates_handle_record_for_divine_test() {
    let app = divine_localnet_admin::app_with_state_for_tests();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/handles")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "name": "alice",
                        "did": "did:plc:alice123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn reads_back_created_handle_record() {
    let app = divine_localnet_admin::app_with_state_for_tests();

    let create = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/handles")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "name": "alice",
                        "did": "did:plc:alice123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/handles/alice")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
