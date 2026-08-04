use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use super::super::{AccountLinkRecord, AppState};

pub async fn handler(
    state: State<AppState>,
    path: Path<String>,
) -> Result<Json<AccountLinkRecord>, StatusCode> {
    super::status::handler(state, path).await
}
