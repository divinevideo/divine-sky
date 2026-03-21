use anyhow::Result;
use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use divine_appview::app_with_store;
use divine_appview::store::{AppviewStore, StoredPost, StoredProfile};
use tower::ServiceExt;

struct FakeStore;

#[async_trait]
impl AppviewStore for FakeStore {
    async fn get_profile(&self, actor: &str) -> Result<Option<StoredProfile>> {
        if actor == "did:plc:ebt5msdpfavoklkap6gl54bm" {
            Ok(Some(StoredProfile {
                did: actor.to_string(),
                handle: "divine.test".to_string(),
                display_name: Some("Divine".to_string()),
                description: Some("Profile".to_string()),
                avatar: None,
                banner: None,
            }))
        } else {
            Ok(None)
        }
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
async fn get_profile_returns_divine_actor_view() {
    let app = app_with_store(FakeStore);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/xrpc/app.bsky.actor.getProfile?actor=did:plc:ebt5msdpfavoklkap6gl54bm")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
