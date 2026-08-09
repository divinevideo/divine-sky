use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use divine_bridge_types::{
    derive_crosspost_status, CrosspostAccountContext, CrosspostAccountSummary, CrosspostJobContext,
    CrosspostStatusResponse,
};
use serde::Deserialize;

use super::super::{AccountLinkRecord, AppState};

const MAX_CROSSPOST_STATUS_BATCH: usize = 100;

#[derive(Debug, Deserialize)]
pub struct CrosspostStatusRequest {
    pub nostr_event_ids: Vec<String>,
}

pub async fn handler(
    State(state): State<AppState>,
    Path(nostr_pubkey): Path<String>,
    Json(payload): Json<CrosspostStatusRequest>,
) -> Result<Json<CrosspostStatusResponse>, StatusCode> {
    if payload.nostr_event_ids.len() > MAX_CROSSPOST_STATUS_BATCH {
        return Err(StatusCode::BAD_REQUEST);
    }

    let account = state
        .get_by_pubkey_result(&nostr_pubkey)
        .await
        .map_err(|error| {
            tracing::error!(
                nostr_pubkey = %nostr_pubkey,
                error = %error,
                "failed to load account link for crosspost status",
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let account_context = account.as_ref().map(account_context);

    let rows = state
        .list_crosspost_status_result(&nostr_pubkey, &payload.nostr_event_ids)
        .await
        .map_err(|error| {
            tracing::error!(
                nostr_pubkey = %nostr_pubkey,
                error = %error,
                "failed to list crosspost status",
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let jobs_by_event_id: HashMap<String, CrosspostJobContext> = rows
        .into_iter()
        .map(|row| {
            (
                row.nostr_event_id,
                CrosspostJobContext {
                    state: row.state,
                    error: row.error,
                    lease_expires_at: row.lease_expires_at,
                    completed_at: row.completed_at,
                    updated_at: row.updated_at,
                    at_uri: row.at_uri,
                    cid: row.cid,
                    record_status: row.record_status,
                },
            )
        })
        .collect();

    let videos = payload
        .nostr_event_ids
        .into_iter()
        .map(|nostr_event_id| {
            let job = jobs_by_event_id.get(&nostr_event_id);
            derive_crosspost_status(nostr_event_id, account_context.as_ref(), job)
        })
        .collect();

    Ok(Json(CrosspostStatusResponse {
        account: CrosspostAccountSummary::from_context(account_context.as_ref()),
        videos,
    }))
}

fn account_context(record: &AccountLinkRecord) -> CrosspostAccountContext {
    CrosspostAccountContext {
        crosspost_enabled: record.crosspost_enabled,
        provisioning_state: match &record.provisioning_state {
            super::super::ProvisioningState::Pending => "pending",
            super::super::ProvisioningState::Ready => "ready",
            super::super::ProvisioningState::Failed => "failed",
            super::super::ProvisioningState::Disabled => "disabled",
        }
        .to_string(),
        did: record.did.clone(),
    }
}
