use anyhow::Result;
use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use divine_appview::app_with_store;
use divine_appview::store::{AppviewStore, StoredPost, StoredProfile};
use tower::ServiceExt;

struct FakeStore;

#[async_trait]
impl AppviewStore for FakeStore {
    async fn get_profile(&self, _actor: &str) -> Result<Option<StoredProfile>> {
        Ok(None)
    }

    async fn get_posts(&self, _uris: &[String]) -> Result<Vec<StoredPost>> {
        Ok(vec![])
    }

    async fn get_post(&self, _uri: &str) -> Result<Option<StoredPost>> {
        Ok(None)
    }

    async fn get_author_feed(
        &self,
        _actor: &str,
        _limit: usize,
        _cursor: Option<String>,
    ) -> Result<Vec<StoredPost>> {
        Ok(vec![])
    }

    async fn search_posts(
        &self,
        _query: &str,
        _limit: usize,
        _cursor: Option<String>,
    ) -> Result<Vec<StoredPost>> {
        Ok(vec![])
    }

    async fn readiness(&self) -> Result<bool> {
        Ok(true)
    }
}

#[tokio::test]
async fn root_page_exposes_lab_service_docs() {
    let app = app_with_store(FakeStore);

    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();

    assert!(body.contains("Divine AppView Lab"));
    assert!(body.contains("/xrpc/app.bsky.feed.getPosts"));
    assert!(body.contains("lab.divine.video"));
}
