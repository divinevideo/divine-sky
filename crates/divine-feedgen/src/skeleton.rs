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
    use diesel::connection::SimpleConnection;
    use diesel::{Connection, PgConnection};

    // CI and local runs export TEST_DATABASE_URL; require it so every line of
    // this helper executes under coverage (no untaken fallback branch).
    fn test_database_url() -> String {
        std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must be set for feedgen store tests")
    }

    #[tokio::test]
    async fn db_feed_store_from_env_queries_both_feeds() {
        let url = test_database_url();
        let mut conn = PgConnection::establish(&url).expect("test database should be reachable");
        // `appview_posts` lives in the non-idempotent 003 migration; reset it with
        // down-then-up the way the appview integration tests do.
        conn.batch_execute(include_str!(
            "../../../migrations/003_appview_read_model/down.sql"
        ))
        .expect("appview read-model down migration should run");
        conn.batch_execute(include_str!(
            "../../../migrations/003_appview_read_model/up.sql"
        ))
        .expect("appview read-model up migration should run");

        std::env::set_var("DATABASE_URL", &url);
        let store = DbFeedStore::from_env();
        std::env::remove_var("DATABASE_URL");

        let latest = store
            .latest_posts(10)
            .await
            .expect("latest feed query succeeds through the pool");
        let trending = store
            .trending_posts(10)
            .await
            .expect("trending feed query succeeds through the pool");

        assert!(latest.len() <= 10);
        assert!(trending.len() <= 10);
    }
}
