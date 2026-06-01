use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use super::super::{AccountLinkRecord, AppState};

pub async fn handler(
    State(state): State<AppState>,
    Path(nostr_pubkey): Path<String>,
) -> Result<Json<AccountLinkRecord>, StatusCode> {
    let record = state
        .enable_by_pubkey_result(&nostr_pubkey)
        .map_err(|error| {
            tracing::error!(error = %error, "failed to enable account link");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let record = record.ok_or(StatusCode::NOT_FOUND)?;

    state.sync_enabled_state(&record).await.map_err(|error| {
        tracing::error!(error = %error, "failed to sync enabled state");
        StatusCode::BAD_GATEWAY
    })?;

    Ok(Json(record))
}
