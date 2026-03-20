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
) -> (StatusCode, Json<AccountLinkRecord>) {
    let record = state.upsert_pending(payload.nostr_pubkey, payload.handle);
    (StatusCode::ACCEPTED, Json(record))
}
