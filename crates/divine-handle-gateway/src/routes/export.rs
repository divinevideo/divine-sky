use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use super::super::{AccountLinkRecord, AppState};

pub async fn handler(
    State(state): State<AppState>,
    Path(nostr_pubkey): Path<String>,
) -> Result<Json<AccountLinkRecord>, StatusCode> {
    let link = state.get_by_pubkey_result(&nostr_pubkey).map_err(|error| {
        tracing::error!(error = %error, "failed to load account link export");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    link.map(Json).ok_or(StatusCode::NOT_FOUND)
}
