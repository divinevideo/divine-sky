use anyhow::{anyhow, Result};
use async_trait::async_trait;
use diesel::{Connection, PgConnection};
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
    database_url: String,
}

impl DbFeedStore {
    pub fn from_env() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL").expect("DATABASE_URL is required"),
        }
    }

    fn connect(&self) -> Result<PgConnection> {
        Ok(PgConnection::establish(&self.database_url)?)
    }
}

#[async_trait]
impl FeedStore for DbFeedStore {
    async fn latest_posts(&self, limit: usize) -> Result<Vec<String>> {
        let mut conn = self.connect()?;
        Ok(list_latest_appview_posts(&mut conn, limit as i64)?
            .into_iter()
            .map(|post| post.uri)
            .collect())
    }

    async fn trending_posts(&self, limit: usize) -> Result<Vec<String>> {
        let mut conn = self.connect()?;
        Ok(list_trending_appview_posts(&mut conn, limit as i64)?
            .into_iter()
            .map(|post| post.uri)
            .collect())
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
