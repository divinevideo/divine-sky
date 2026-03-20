use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use super::super::{AccountLinkRecord, AppState};

#[derive(Debug, Deserialize)]
pub struct OptInRequest {
    pub nostr_pubkey: String,
    pub handle: String,
}

pub async fn handler(
    State(state): State<AppState>,
    Json(payload): Json<OptInRequest>,
) -> Result<(StatusCode, Json<AccountLinkRecord>), StatusCode> {
    let record = state
        .upsert_pending_result(payload.nostr_pubkey, payload.handle)
        .map_err(|error| {
            tracing::error!(error = %error, "failed to persist pending opt-in");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    state.enqueue_provisioning(&record.nostr_pubkey, &record.handle);
    Ok((StatusCode::ACCEPTED, Json(record)))
}
