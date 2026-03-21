use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::views::{post_view, SearchPostsResponse};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

pub async fn search_posts(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<SearchPostsResponse>, StatusCode> {
    let posts = state
        .store
        .search_posts(&query.q, query.limit.unwrap_or(25), query.cursor)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(SearchPostsResponse {
        posts: posts.into_iter().map(post_view).collect(),
        cursor: None,
    }))
}
