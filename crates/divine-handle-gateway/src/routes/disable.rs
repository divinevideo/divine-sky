use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use super::super::{AccountLinkRecord, AppState};

pub async fn handler(
    State(state): State<AppState>,
    Path(nostr_pubkey): Path<String>,
) -> Result<Json<AccountLinkRecord>, StatusCode> {
    let record = state
        .disable_by_pubkey_result(&nostr_pubkey)
        .map_err(|error| {
            tracing::error!(error = %error, "failed to disable account link");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let record = record.ok_or(StatusCode::NOT_FOUND)?;

    state
        .sync_disabled_state(&record.nostr_pubkey, &record.handle)
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "failed to sync disabled state");
            StatusCode::BAD_GATEWAY
        })?;

    Ok(Json(record))
}
