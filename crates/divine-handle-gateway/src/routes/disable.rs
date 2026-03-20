use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use super::super::{AccountLinkRecord, AppState};

pub async fn handler(
    State(state): State<AppState>,
    Path(nostr_pubkey): Path<String>,
) -> Result<Json<AccountLinkRecord>, StatusCode> {
    state
        .disable_by_pubkey(&nostr_pubkey)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}
