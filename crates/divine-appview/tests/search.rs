use anyhow::Result;
use async_trait::async_trait;
use axum::body::Body;
use axum::http::Request;
use divine_appview::app_with_store;
use divine_appview::store::{AppviewStore, StoredPost, StoredProfile};
use tower::ServiceExt;

#[derive(Clone)]
struct FakeStore {
    post: StoredPost,
}

#[async_trait]
impl AppviewStore for FakeStore {
    async fn get_profile(&self, _actor: &str) -> Result<Option<StoredProfile>> {
        Ok(Some(StoredProfile {
            did: self.post.did.clone(),
            handle: self.post.handle.clone(),
            display_name: self.post.display_name.clone(),
            description: self.post.description.clone(),
            avatar: None,
            banner: None,
        }))
    }

    async fn get_posts(&self, _uris: &[String]) -> Result<Vec<StoredPost>> {
        Ok(vec![self.post.clone()])
    }

    async fn get_post(&self, _uri: &str) -> Result<Option<StoredPost>> {
        Ok(Some(self.post.clone()))
    }

    async fn get_author_feed(
        &self,
        _actor: &str,
        _limit: usize,
        _cursor: Option<String>,
    ) -> Result<Vec<StoredPost>> {
        Ok(vec![self.post.clone()])
    }

    async fn search_posts(
        &self,
        _query: &str,
        _limit: usize,
        _cursor: Option<String>,
    ) -> Result<Vec<StoredPost>> {
        Ok(vec![self.post.clone()])
    }

    async fn readiness(&self) -> Result<bool> {
        Ok(true)
    }
}

#[tokio::test]
async fn search_posts_returns_video_views_without_raw_blob_urls() {
    let app = app_with_store(FakeStore {
        post: StoredPost {
            uri: "at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/MA6mjTWZKEB".to_string(),
            cid: Some("cid-demo".to_string()),
            did: "did:plc:ebt5msdpfavoklkap6gl54bm".to_string(),
            handle: "divine.test".to_string(),
            display_name: Some("Divine".to_string()),
            description: Some("Profile".to_string()),
            avatar: None,
            banner: None,
            text: "Divine video".to_string(),
            created_at: chrono::Utc::now(),
            embed_blob_cid: Some("bafkrei-demo".to_string()),
            embed_alt: Some("Demo".to_string()),
            playlist_url: Some("https://media.divine.test/playlists/bafkrei-demo.m3u8".to_string()),
            thumbnail_url: Some("https://media.divine.test/thumb.jpg".to_string()),
        },
    });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/xrpc/app.bsky.feed.searchPosts?q=divine")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();

    assert!(text.contains("\"playlist\""));
    assert!(!text.contains("com.atproto.sync.getBlob"));
}
