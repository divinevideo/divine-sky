use anyhow::Result;
use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use divine_appview::app_with_store;
use divine_appview::store::{AppviewStore, StoredPost, StoredProfile};
use tower::ServiceExt;

#[derive(Clone)]
struct FakeStore {
    post: StoredPost,
}

impl FakeStore {
    fn with_video_post(uri: &str, playlist_url: &str) -> Self {
        Self {
            post: StoredPost {
                uri: uri.to_string(),
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
                playlist_url: Some(playlist_url.to_string()),
                thumbnail_url: Some("https://media.divine.test/thumb.jpg".to_string()),
            },
        }
    }
}

#[async_trait]
impl AppviewStore for FakeStore {
    async fn get_profile(&self, _actor: &str) -> Result<Option<StoredProfile>> {
        Ok(Some(StoredProfile {
            did: self.post.did.clone(),
            handle: self.post.handle.clone(),
            display_name: self.post.display_name.clone(),
            description: self.post.description.clone(),
            avatar: self.post.avatar.clone(),
            banner: self.post.banner.clone(),
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
async fn get_posts_hydrates_video_embed_view() {
    let app = app_with_store(FakeStore::with_video_post(
        "at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/MA6mjTWZKEB",
        "https://media.divine.test/playlists/bafkrei-demo.m3u8",
    ));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/xrpc/app.bsky.feed.getPosts?uris=at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/MA6mjTWZKEB")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
