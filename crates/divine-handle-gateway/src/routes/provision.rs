use axum::extract::State;
use axum::Json;
use serde::Deserialize;

use super::super::{AccountLinkRecord, AppState};

#[derive(Debug, Deserialize)]
pub struct ProvisionRequest {
    pub nostr_pubkey: String,
    pub handle: String,
    pub did: String,
}

pub async fn handler(
    State(state): State<AppState>,
    Json(payload): Json<ProvisionRequest>,
) -> Json<AccountLinkRecord> {
    Json(state.upsert_ready(payload.nostr_pubkey, payload.handle, payload.did))
}
