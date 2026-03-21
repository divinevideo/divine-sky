use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::views::profile_view;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ProfileQuery {
    pub actor: String,
}

pub async fn get_profile(
    State(state): State<AppState>,
    Query(query): Query<ProfileQuery>,
) -> Result<Json<crate::views::ProfileView>, StatusCode> {
    let profile = state
        .store
        .get_profile(&query.actor)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(profile_view(profile)))
}
