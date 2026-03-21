use axum::extract::{Query, RawQuery, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::views::{
    post_view, FeedItem, FeedResponse, GetPostsResponse, PostThreadResponse, ThreadView,
};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct AuthorFeedQuery {
    pub actor: String,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PostThreadQuery {
    pub uri: String,
}

pub async fn get_author_feed(
    State(state): State<AppState>,
    Query(query): Query<AuthorFeedQuery>,
) -> Result<Json<FeedResponse>, StatusCode> {
    let posts = state
        .store
        .get_author_feed(&query.actor, query.limit.unwrap_or(25), query.cursor)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(FeedResponse {
        feed: posts
            .into_iter()
            .map(|post| FeedItem {
                post: post_view(post),
            })
            .collect(),
        cursor: None,
    }))
}

pub async fn get_posts(
    State(state): State<AppState>,
    raw_query: RawQuery,
) -> Result<Json<GetPostsResponse>, StatusCode> {
    let uris = raw_query
        .0
        .as_deref()
        .map(parse_uris_query)
        .unwrap_or_default();
    let posts = state
        .store
        .get_posts(&uris)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(GetPostsResponse {
        posts: posts.into_iter().map(post_view).collect(),
    }))
}

pub async fn get_post_thread(
    State(state): State<AppState>,
    Query(query): Query<PostThreadQuery>,
) -> Result<Json<PostThreadResponse>, StatusCode> {
    let post = state
        .store
        .get_post(&query.uri)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(PostThreadResponse {
        thread: ThreadView {
            post: post_view(post),
        },
    }))
}

fn parse_uris_query(query: &str) -> Vec<String> {
    url::form_urlencoded::parse(query.as_bytes())
        .filter_map(|(key, value)| (key == "uris").then(|| value.into_owned()))
        .collect()
}
