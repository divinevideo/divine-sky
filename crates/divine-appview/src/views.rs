use chrono::SecondsFormat;
use serde::Serialize;

use crate::store::{StoredPost, StoredProfile};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    pub did: String,
    pub handle: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub avatar: Option<String>,
    pub banner: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GetPostsResponse {
    pub posts: Vec<PostView>,
}

#[derive(Debug, Serialize)]
pub struct FeedResponse {
    pub feed: Vec<FeedItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FeedItem {
    pub post: PostView,
}

#[derive(Debug, Serialize)]
pub struct SearchPostsResponse {
    pub posts: Vec<PostView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PostThreadResponse {
    pub thread: ThreadView,
}

#[derive(Debug, Serialize)]
pub struct ThreadView {
    pub post: PostView,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostView {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    pub author: ProfileView,
    pub text: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed: Option<VideoEmbedView>,
}

#[derive(Debug, Serialize)]
pub struct VideoEmbedView {
    #[serde(rename = "$type")]
    pub type_: String,
    pub cid: String,
    pub playlist: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
}

pub fn profile_view(profile: StoredProfile) -> ProfileView {
    ProfileView {
        did: profile.did,
        handle: profile.handle,
        display_name: profile.display_name,
        description: profile.description,
        avatar: profile.avatar,
        banner: profile.banner,
    }
}

pub fn post_view(post: StoredPost) -> PostView {
    let embed = match (post.embed_blob_cid.clone(), post.playlist_url.clone()) {
        (Some(cid), Some(playlist)) => Some(VideoEmbedView {
            type_: "app.bsky.embed.video#view".to_string(),
            cid,
            playlist,
            thumbnail: post.thumbnail_url,
            alt: post.embed_alt,
        }),
        _ => None,
    };

    PostView {
        uri: post.uri,
        cid: post.cid,
        author: ProfileView {
            did: post.did,
            handle: post.handle,
            display_name: post.display_name,
            description: post.description,
            avatar: post.avatar,
            banner: post.banner,
        },
        text: post.text,
        created_at: post.created_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        embed,
    }
}
