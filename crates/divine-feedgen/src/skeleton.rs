use anyhow::{anyhow, Result};
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

pub fn feed_skeleton(feed: &str) -> Result<FeedSkeletonResponse> {
    let items = match feed {
        LATEST_URI => latest_posts(),
        TRENDING_URI => trending_posts(),
        _ => return Err(anyhow!("unknown feed URI: {feed}")),
    };

    Ok(FeedSkeletonResponse {
        feed: items.into_iter().map(|post| FeedItem { post }).collect(),
        cursor: None,
    })
}

fn latest_posts() -> Vec<String> {
    vec![
        "at://did:plc:divine.creator/app.bsky.feed.post/latest-001".to_string(),
        "at://did:plc:divine.creator/app.bsky.feed.post/latest-002".to_string(),
        "at://did:plc:divine.creator/app.bsky.feed.post/latest-003".to_string(),
    ]
}

fn trending_posts() -> Vec<String> {
    vec![
        "at://did:plc:divine.rank/app.bsky.feed.post/trending-900".to_string(),
        "at://did:plc:divine.rank/app.bsky.feed.post/trending-650".to_string(),
        "at://did:plc:divine.rank/app.bsky.feed.post/trending-420".to_string(),
    ]
}
