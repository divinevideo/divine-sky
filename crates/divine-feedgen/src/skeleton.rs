use anyhow::{anyhow, Result};
use async_trait::async_trait;
use divine_bridge_db::pool::{build_pool, DbPool};
use divine_bridge_db::{list_latest_appview_posts, list_trending_appview_posts};
use serde::Serialize;
use std::sync::Arc;

const FEED_DID: &str = "did:plc:divine.feed";
const LATEST_URI: &str = "at://did:plc:divine.feed/app.bsky.feed.generator/latest";
const TRENDING_URI: &str = "at://did:plc:divine.feed/app.bsky.feed.generator/trending";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedDescriptor {
    pub uri: String,
    pub display_name: String,
    pub description: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeFeedGeneratorResponse {
    pub did: String,
    pub feeds: Vec<FeedDescriptor>,
}

#[derive(Debug, Serialize)]
pub struct FeedItem {
    pub post: String,
}

#[derive(Debug, Serialize)]
pub struct FeedSkeletonResponse {
    pub feed: Vec<FeedItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[async_trait]
pub trait FeedStore: Send + Sync {
    async fn latest_posts(&self, limit: usize) -> Result<Vec<String>>;
    async fn trending_posts(&self, limit: usize) -> Result<Vec<String>>;
}

pub type DynFeedStore = Arc<dyn FeedStore>;

pub struct DbFeedStore {
    pool: DbPool,
}

impl DbFeedStore {
    pub fn from_env() -> Self {
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL is required");
        Self {
            pool: build_pool(&database_url).expect("failed to build feedgen database pool"),
        }
    }
}

#[async_trait]
impl FeedStore for DbFeedStore {
    async fn latest_posts(&self, limit: usize) -> Result<Vec<String>> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get()?;
            Ok(list_latest_appview_posts(&mut conn, limit as i64)?
                .into_iter()
                .map(|post| post.uri)
                .collect())
        })
        .await?
    }

    async fn trending_posts(&self, limit: usize) -> Result<Vec<String>> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get()?;
            Ok(list_trending_appview_posts(&mut conn, limit as i64)?
                .into_iter()
                .map(|post| post.uri)
                .collect())
        })
        .await?
    }
}

pub fn describe_feed_generator() -> DescribeFeedGeneratorResponse {
    DescribeFeedGeneratorResponse {
        did: FEED_DID.to_string(),
        feeds: vec![
            FeedDescriptor {
                uri: LATEST_URI.to_string(),
                display_name: "DiVine Latest".to_string(),
                description: "Latest DiVine-owned mirrored videos.".to_string(),
            },
            FeedDescriptor {
                uri: TRENDING_URI.to_string(),
                display_name: "DiVine Trending".to_string(),
                description: "Ranked DiVine-owned mirrored videos.".to_string(),
            },
        ],
    }
}

pub async fn feed_skeleton(
    store: &dyn FeedStore,
    feed: &str,
    limit: usize,
) -> Result<FeedSkeletonResponse> {
    let items = match feed {
        LATEST_URI => store.latest_posts(limit).await?,
        TRENDING_URI => store.trending_posts(limit).await?,
        _ => return Err(anyhow!("unknown feed URI: {feed}")),
    };

    Ok(FeedSkeletonResponse {
        feed: items.into_iter().map(|post| FeedItem { post }).collect(),
        cursor: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    struct EmptyFeedStore;

    #[async_trait]
    impl FeedStore for EmptyFeedStore {
        async fn latest_posts(&self, limit: usize) -> Result<Vec<String>> {
            Ok(vec![format!(
                "at://did:plc:test/app.bsky.feed.post/latest-{limit}"
            )])
        }

        async fn trending_posts(&self, limit: usize) -> Result<Vec<String>> {
            Ok(vec![format!(
                "at://did:plc:test/app.bsky.feed.post/trending-{limit}"
            )])
        }
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[tokio::test]
    async fn feed_skeleton_rejects_unknown_feed_uri() {
        let error = feed_skeleton(&EmptyFeedStore, "at://did:plc:divine.feed/unknown", 10)
            .await
            .expect_err("unknown feed should fail");

        assert!(error.to_string().contains("unknown feed URI"));
    }

    #[tokio::test]
    async fn feed_skeleton_returns_latest_posts() {
        let response = feed_skeleton(&EmptyFeedStore, LATEST_URI, 3)
            .await
            .expect("latest feed should resolve");

        assert_eq!(response.feed.len(), 1);
        assert_eq!(
            response.feed[0].post,
            "at://did:plc:test/app.bsky.feed.post/latest-3"
        );
        assert!(response.cursor.is_none());
    }

    #[tokio::test]
    async fn feed_skeleton_returns_trending_posts() {
        let response = feed_skeleton(&EmptyFeedStore, TRENDING_URI, 7)
            .await
            .expect("trending feed should resolve");

        assert_eq!(response.feed.len(), 1);
        assert_eq!(
            response.feed[0].post,
            "at://did:plc:test/app.bsky.feed.post/trending-7"
        );
        assert!(response.cursor.is_none());
    }

    #[test]
    #[should_panic(expected = "DATABASE_URL is required")]
    fn db_feed_store_from_env_requires_database_url() {
        let guard = env_lock().lock().unwrap();
        std::env::remove_var("DATABASE_URL");
        DbFeedStore::from_env();
        drop(guard);
    }
}
