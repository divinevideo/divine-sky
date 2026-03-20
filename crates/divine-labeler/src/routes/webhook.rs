//! POST /webhook/moderation-result
//!
//! Receives moderation results from the JS moderation service,
//! maps to ATProto labels, signs them, and stores in labeler_events.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use divine_bridge_db::models::NewLabelerEvent;
use divine_moderation_adapter::labels::vocabulary::divine_to_atproto;

use crate::signing::{sign_label, UnsignedLabel};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct WebhookPayload {
    pub sha256: String,
    pub action: String,
    pub labels: Vec<LabelScore>,
    pub reviewed_by: Option<String>,
    pub timestamp: Option<String>,
    pub nostr_event_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LabelScore {
    pub category: String,
    pub score: f64,
}

#[derive(Debug, Serialize)]
pub struct WebhookResponse {
    accepted: usize,
    errors: Vec<String>,
}

pub async fn handler(
    State(state): State<AppState>,
    Json(payload): Json<WebhookPayload>,
) -> Result<Json<WebhookResponse>, StatusCode> {
    let mut accepted = 0;
    let mut errors = Vec::new();
    let now = Utc::now();
    let cts = now.to_rfc3339();

    // Resolve AT URI from record_mappings if nostr_event_id is provided
    let at_uri = if let Some(ref event_id) = payload.nostr_event_id {
        match state.store.get_at_uri_by_event_id(event_id) {
            Ok(Some((uri, _did))) => uri,
            _ => format!("at://sha256:{}", payload.sha256),
        }
    } else {
        format!("at://sha256:{}", payload.sha256)
    };

    for label_score in &payload.labels {
        let atproto_val = match divine_to_atproto(&label_score.category) {
            Some(v) => v,
            None => {
                errors.push(format!("unknown category: {}", label_score.category));
                continue;
            }
        };

        let unsigned = UnsignedLabel {
            ver: 1,
            src: state.labeler_did.clone(),
            uri: at_uri.clone(),
            cid: None,
            val: atproto_val.to_string(),
            neg: false,
            cts: cts.clone(),
        };

        let sig = match sign_label(&unsigned, &state.signing_key) {
            Ok(s) => s,
            Err(e) => {
                errors.push(format!("signing failed for {}: {e}", atproto_val));
                continue;
            }
        };

        let new_event = NewLabelerEvent {
            src_did: &state.labeler_did,
            subject_uri: &unsigned.uri,
            subject_cid: None,
            val: atproto_val,
            neg: false,
            nostr_event_id: None,
            sha256: Some(&payload.sha256),
            origin: if payload.reviewed_by.is_some() { "human" } else { "divine" },
        };

        match state.store.insert_labeler_event(&new_event) {
            Ok(_event) => {
                tracing::info!(
                    sha256 = %payload.sha256,
                    val = atproto_val,
                    "label emitted (sig={} bytes)", sig.len()
                );
                accepted += 1;
            }
            Err(e) => {
                errors.push(format!("db insert failed for {}: {e}", atproto_val));
            }
        }
    }

    // PERMANENT_BAN also emits a !takedown label
    if payload.action == "PERMANENT_BAN" {
        let unsigned = UnsignedLabel {
            ver: 1,
            src: state.labeler_did.clone(),
            uri: at_uri.clone(),
            cid: None,
            val: "!takedown".to_string(),
            neg: false,
            cts: cts.clone(),
        };

        if let Ok(_sig) = sign_label(&unsigned, &state.signing_key) {
            let new_event = NewLabelerEvent {
                src_did: &state.labeler_did,
                subject_uri: &unsigned.uri,
                subject_cid: None,
                val: "!takedown",
                neg: false,
                nostr_event_id: None,
                sha256: Some(&payload.sha256),
                origin: "divine",
            };

            match state.store.insert_labeler_event(&new_event) {
                Ok(_) => {
                    tracing::info!(sha256 = %payload.sha256, "!takedown label emitted");
                    accepted += 1;
                }
                Err(e) => errors.push(format!("db insert failed for !takedown: {e}")),
            }
        }
    }

    Ok(Json(WebhookResponse { accepted, errors }))
}
