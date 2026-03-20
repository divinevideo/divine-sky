//! GET /xrpc/com.atproto.label.queryLabels

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::{Deserialize, Serialize};

use divine_bridge_db::models::LabelerEvent;

use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct QueryParams {
    #[serde(rename = "uriPatterns")]
    pub uri_patterns: Option<String>,
    pub sources: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize)]
struct LabelOutput {
    ver: u32,
    src: String,
    uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cid: Option<String>,
    val: String,
    neg: bool,
    cts: String,
}

#[derive(Debug, Serialize)]
pub struct QueryLabelsResponse {
    labels: Vec<LabelOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cursor: Option<String>,
}

/// Build query response from labeler events. Exported for testing.
pub fn build_query_response(events: &[LabelerEvent]) -> (String, Option<String>) {
    let cursor = events.last().map(|e| e.seq.to_string());

    let labels: Vec<LabelOutput> = events
        .iter()
        .map(|e| LabelOutput {
            ver: 1,
            src: e.src_did.clone(),
            uri: e.subject_uri.clone(),
            cid: e.subject_cid.clone(),
            val: e.val.clone(),
            neg: e.neg,
            cts: e.created_at.to_rfc3339(),
        })
        .collect();

    let response = QueryLabelsResponse {
        labels,
        cursor: cursor.clone(),
    };

    let body = serde_json::to_string(&response).unwrap_or_else(|_| r#"{"labels":[]}"#.to_string());
    (body, cursor)
}

pub async fn handler(
    State(state): State<AppState>,
    Query(params): Query<QueryParams>,
) -> Result<Json<QueryLabelsResponse>, StatusCode> {
    let limit = params.limit.unwrap_or(50).min(250);
    let after_seq = params
        .cursor
        .as_deref()
        .and_then(|c| c.parse::<i64>().ok())
        .unwrap_or(0);

    let events = state
        .store
        .get_events_after(after_seq, limit)
        .map_err(|e| {
            tracing::error!("failed to query labeler events: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Apply URI pattern filtering if specified
    let uri_patterns: Vec<String> = params
        .uri_patterns
        .map(|p| p.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    let filtered: Vec<&LabelerEvent> = if uri_patterns.is_empty() {
        events.iter().collect()
    } else {
        events
            .iter()
            .filter(|e| {
                uri_patterns.iter().any(|p| {
                    if let Some(prefix) = p.strip_suffix('*') {
                        e.subject_uri.starts_with(prefix)
                    } else {
                        e.subject_uri == *p
                    }
                })
            })
            .collect()
    };

    let cursor = filtered.last().map(|e| e.seq.to_string());

    let labels: Vec<LabelOutput> = filtered
        .iter()
        .map(|e| LabelOutput {
            ver: 1,
            src: e.src_did.clone(),
            uri: e.subject_uri.clone(),
            cid: e.subject_cid.clone(),
            val: e.val.clone(),
            neg: e.neg,
            cts: e.created_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(QueryLabelsResponse { labels, cursor }))
}
