mod skeleton;
pub use skeleton::FeedStore;

use std::sync::Arc;

use axum::extract::Query;
use axum::response::Html;
use axum::routing::get;
use axum::{http::StatusCode, Json, Router};
use serde::Deserialize;
use tower_http::cors::CorsLayer;

#[derive(Debug, Deserialize)]
struct FeedQuery {
    feed: String,
    limit: Option<usize>,
}

#[derive(Clone)]
struct AppState {
    store: skeleton::DynFeedStore,
}

async fn describe_feed_generator() -> Json<skeleton::DescribeFeedGeneratorResponse> {
    Json(skeleton::describe_feed_generator())
}

async fn get_feed_skeleton(
    axum::extract::State(state): axum::extract::State<AppState>,
    Query(query): Query<FeedQuery>,
) -> Result<Json<skeleton::FeedSkeletonResponse>, axum::http::StatusCode> {
    skeleton::feed_skeleton(state.store.as_ref(), &query.feed, query.limit.unwrap_or(25))
        .await
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
    app_with_store_and_viewer_origin(
        skeleton::DbFeedStore::from_env(),
        std::env::var("VIEWER_ORIGIN").ok(),
    )
}

pub fn app_with_store<S>(store: S) -> Router
where
    S: skeleton::FeedStore + 'static,
{
    app_with_store_and_viewer_origin(store, None)
}

pub fn app_with_store_and_viewer_origin<S>(store: S, viewer_origin: Option<String>) -> Router
where
    S: skeleton::FeedStore + 'static,
{
    let cors = match viewer_origin {
        Some(origin) => CorsLayer::very_permissive().allow_origin([origin.parse().unwrap()]),
        None => CorsLayer::permissive(),
    };

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
        .with_state(AppState {
            store: Arc::new(store),
        })
        .layer(cors)
}
