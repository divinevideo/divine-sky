use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::Serialize;

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

#[derive(Clone, Debug, Default)]
pub struct DbFeedStore;

impl DbFeedStore {
    pub fn from_env() -> Self {
        Self
    }
}

#[async_trait]
impl FeedStore for DbFeedStore {
    async fn latest_posts(&self, limit: usize) -> Result<Vec<String>> {
        Ok(latest_posts().into_iter().take(limit).collect())
    }

    async fn trending_posts(&self, limit: usize) -> Result<Vec<String>> {
        Ok(trending_posts().into_iter().take(limit).collect())
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

/// Returns the current latest feed URIs used by the local feed generator.
fn latest_posts() -> Vec<String> {
    vec![
        "at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/3mhjk5tbom655".to_string(),
        "at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/3mhjk3ct6xja5".to_string(),
        "at://did:plc:w2bvwfebcrmc2pznxvz3lfdi/app.bsky.feed.post/3mhjn3iejoaaa".to_string(),
        "at://did:plc:w2bvwfebcrmc2pznxvz3lfdi/app.bsky.feed.post/3mhjmzie5xmtk".to_string(),
    ]
}

fn trending_posts() -> Vec<String> {
    // Same posts for now; trending and latest share the same backing list.
    latest_posts()
}
