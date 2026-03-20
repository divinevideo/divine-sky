mod skeleton;

use axum::extract::Query;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct FeedQuery {
    feed: String,
}

async fn describe_feed_generator() -> Json<skeleton::DescribeFeedGeneratorResponse> {
    Json(skeleton::describe_feed_generator())
}

async fn get_feed_skeleton(
    Query(query): Query<FeedQuery>,
) -> Result<Json<skeleton::FeedSkeletonResponse>, axum::http::StatusCode> {
    skeleton::feed_skeleton(&query.feed)
        .map(Json)
        .map_err(|_| axum::http::StatusCode::NOT_FOUND)
}

pub fn app() -> Router {
    Router::new()
        .route(
            "/xrpc/app.bsky.feed.describeFeedGenerator",
            get(describe_feed_generator),
        )
        .route(
            "/xrpc/app.bsky.feed.getFeedSkeleton",
            get(get_feed_skeleton),
        )
}
