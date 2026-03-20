use axum::body::Body;
use axum::http::{Request, StatusCode};
use divine_feedgen::app;
use serde_json::Value;
use tower::util::ServiceExt;

#[tokio::test]
async fn feed_skeleton_describes_latest_and_trending_feeds() {
    let app = app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/xrpc/app.bsky.feed.describeFeedGenerator")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn feed_skeleton_latest_returns_divine_owned_posts() {
    let app = app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:plc:divine.feed/app.bsky.feed.generator/latest")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    let first_post = json["feed"][0]["post"].as_str().unwrap();
    assert!(first_post.starts_with("at://did:plc:divine."));
}

#[tokio::test]
async fn feed_skeleton_trending_returns_distinct_ranked_posts() {
    let app = app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:plc:divine.feed/app.bsky.feed.generator/trending")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    let feed = json["feed"].as_array().unwrap();
    assert!(feed.len() >= 2);
    assert_ne!(feed[0]["post"], feed[1]["post"]);
}
