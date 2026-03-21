use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use tower_http::cors::CorsLayer;

pub mod config;
pub mod routes;
pub mod store;
pub mod views;

use store::{AppviewStore, DbStore, DynStore};

#[derive(Clone)]
pub struct AppState {
    pub store: DynStore,
}

pub fn app_with_store<S>(store: S) -> Router
where
    S: AppviewStore + 'static,
{
    app_with_dyn_store(Arc::new(store))
}

pub fn app_from_config(config: config::AppviewConfig) -> Router {
    let store = DbStore::new(config.database_url, config.media_base_url);
    let cors = match config.viewer_origin {
        Some(origin) => CorsLayer::very_permissive().allow_origin([origin.parse().unwrap()]),
        None => CorsLayer::permissive(),
    };

    app_with_dyn_store(Arc::new(store)).layer(cors)
}

fn app_with_dyn_store(store: DynStore) -> Router {
    let state = AppState { store };

    Router::new()
        .route("/", get(routes::root::root_info))
        .route("/health", get(routes::health::health))
        .route("/health/ready", get(routes::health::ready))
        .route(
            "/xrpc/app.bsky.actor.getProfile",
            get(routes::actor::get_profile),
        )
        .route(
            "/xrpc/app.bsky.feed.getAuthorFeed",
            get(routes::feed::get_author_feed),
        )
        .route("/xrpc/app.bsky.feed.getPosts", get(routes::feed::get_posts))
        .route(
            "/xrpc/app.bsky.feed.getPostThread",
            get(routes::feed::get_post_thread),
        )
        .route(
            "/xrpc/app.bsky.feed.searchPosts",
            get(routes::search::search_posts),
        )
        .with_state(state)
}
