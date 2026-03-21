mod skeleton;

use axum::extract::Query;
use axum::response::Html;
use axum::routing::get;
use axum::{http::StatusCode, Json, Router};
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

const ROOT_HTML: &str = include_str!("root_page.html");

async fn root_info() -> Html<&'static str> {
    Html(ROOT_HTML)
}

async fn health() -> StatusCode {
    StatusCode::OK
}

async fn health_ready() -> StatusCode {
    StatusCode::OK
}

pub fn app() -> Router {
    Router::new()
        .route("/", get(root_info))
        .route("/health", get(health))
        .route("/health/ready", get(health_ready))
        .route(
            "/xrpc/app.bsky.feed.describeFeedGenerator",
            get(describe_feed_generator),
        )
        .route(
            "/xrpc/app.bsky.feed.getFeedSkeleton",
            get(get_feed_skeleton),
        )
}
