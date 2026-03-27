use axum::extract::State;
use axum::http::StatusCode;
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
) -> Result<Json<AccountLinkRecord>, StatusCode> {
    let record = state.upsert_ready(
        payload.nostr_pubkey.clone(),
        payload.handle.clone(),
        payload.did.clone(),
    );

    state
        .sync_ready_state(&payload.nostr_pubkey, &payload.handle, &payload.did)
        .await
        .map_err(|error| {
            tracing::error!(
                nostr_pubkey = %payload.nostr_pubkey,
                handle = %payload.handle,
                did = %payload.did,
                error = %error,
                "failed to sync manual provision ready state downstream",
            );
            StatusCode::BAD_GATEWAY
        })?;

    Ok(Json(record))
}
