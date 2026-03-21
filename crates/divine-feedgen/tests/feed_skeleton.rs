use anyhow::Result;
use async_trait::async_trait;
use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use divine_feedgen::{app_with_store, app_with_store_and_viewer_origin, FeedStore};
use serde_json::Value;
use tower::util::ServiceExt;

struct FakeFeedStore {
    latest: Vec<String>,
    trending: Vec<String>,
}

#[async_trait]
impl FeedStore for FakeFeedStore {
    async fn latest_posts(&self, _limit: usize) -> Result<Vec<String>> {
        Ok(self.latest.clone())
    }

    async fn trending_posts(&self, _limit: usize) -> Result<Vec<String>> {
        Ok(self.trending.clone())
    }
}

#[tokio::test]
async fn feed_skeleton_describes_latest_and_trending_feeds() {
    let app = app_with_store(FakeFeedStore {
        latest: vec![],
        trending: vec![],
    });

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
async fn feedgen_health_endpoints_return_ok() {
    let app = app_with_store(FakeFeedStore {
        latest: vec![],
        trending: vec![],
    });

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
async fn feedgen_root_page_surfaces_lab_docs() {
    let app = app_with_store(FakeFeedStore {
        latest: vec![],
        trending: vec![],
    });

    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();

    assert!(body.contains("Divine Blacksky Feed Generator"));
    assert!(body.contains("Viewer Lab"));
    assert!(body.contains("at://did:plc:divine.feed/app.bsky.feed.generator/latest"));
}

#[tokio::test]
async fn feed_skeleton_latest_reads_indexed_posts() {
    let app = app_with_store(FakeFeedStore {
        latest: vec![
            "at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/MA6mjTWZKEB".to_string(),
            "at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/hFxlUuKIIqU".to_string(),
        ],
        trending: vec![],
    });

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
    assert_eq!(
        first_post,
        "at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/MA6mjTWZKEB"
    );
}

#[tokio::test]
async fn feed_skeleton_trending_reads_indexed_posts() {
    let app = app_with_store(FakeFeedStore {
        latest: vec![],
        trending: vec![
            "at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/hFxlUuKIIqU".to_string(),
            "at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/MA6mjTWZKEB".to_string(),
        ],
    });

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
    assert_eq!(
        feed[0]["post"].as_str().unwrap(),
        "at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/hFxlUuKIIqU"
    );
}

#[tokio::test]
async fn feedgen_allows_configured_viewer_origin_for_browser_requests() {
    let app = app_with_store_and_viewer_origin(
        FakeFeedStore {
            latest: vec![
                "at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/MA6mjTWZKEB".to_string(),
            ],
            trending: vec![],
        },
        Some("http://127.0.0.1:4173".to_string()),
    );

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:plc:divine.feed/app.bsky.feed.generator/latest")
                .header(header::ORIGIN, "http://127.0.0.1:4173")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&header::HeaderValue::from_static("http://127.0.0.1:4173"))
    );
}
