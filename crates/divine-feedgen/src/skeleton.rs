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

/// Returns real post URIs from test accounts on pds.staging.dvines.org.
/// In production, these would come from a database query or ClickHouse.
fn latest_posts() -> Vec<String> {
    vec![
        // Posts from bridgefinal.staging.dvines.org (did:plc:ebt5msdpfavoklkap6gl54bm)
        "at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/3mhjk5tbom655".to_string(),
        "at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/3mhjk3ct6xja5".to_string(),
        // Posts from divinetest.pds.staging.dvines.org (did:plc:w2bvwfebcrmc2pznxvz3lfdi)
        "at://did:plc:w2bvwfebcrmc2pznxvz3lfdi/app.bsky.feed.post/3mhjn3iejoaaa".to_string(),
        "at://did:plc:w2bvwfebcrmc2pznxvz3lfdi/app.bsky.feed.post/3mhjmzie5xmtk".to_string(),
    ]
}

fn trending_posts() -> Vec<String> {
    // Same posts for now — trending algorithm not yet implemented
    latest_posts()
}
