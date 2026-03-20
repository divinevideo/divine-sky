//! ATProto labeler service endpoint formatters.
//!
//! The actual HTTP/WebSocket server depends on the runtime (Axum/Rocket).
//! This module provides the query logic and response formatting.

use serde::{Deserialize, Serialize};

/// A label as stored in labeler_events, ready to be served.
#[derive(Debug, Clone, Serialize)]
pub struct StoredLabel {
    pub seq: i64,
    pub src_did: String,
    pub subject_uri: String,
    pub subject_cid: Option<String>,
    pub val: String,
    pub neg: bool,
    pub created_at: String,
}

/// Parameters for com.atproto.label.queryLabels.
#[derive(Debug, Clone, Deserialize)]
pub struct QueryLabelsParams {
    #[serde(rename = "uriPatterns")]
    pub uri_patterns: Option<Vec<String>>,
    pub sources: Option<Vec<String>>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    pub cursor: Option<String>,
}

fn default_limit() -> i64 {
    50
}

impl QueryLabelsParams {
    pub fn matches_uri(&self, uri: &str) -> bool {
        match &self.uri_patterns {
            None => true,
            Some(patterns) => patterns.iter().any(|p| {
                if let Some(prefix) = p.strip_suffix('*') {
                    uri.starts_with(prefix)
                } else {
                    uri == p
                }
            }),
        }
    }

    pub fn matches_source(&self, src: &str) -> bool {
        match &self.sources {
            None => true,
            Some(sources) => sources.iter().any(|s| s == src),
        }
    }
}

/// Format labels into a com.atproto.label.queryLabels response body.
pub fn format_query_labels_response(labels: &[StoredLabel], cursor: Option<&str>) -> String {
    #[derive(Serialize)]
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

    #[derive(Serialize)]
    struct Response {
        labels: Vec<LabelOutput>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
    }

    let output = Response {
        labels: labels
            .iter()
            .map(|l| LabelOutput {
                ver: 1,
                src: l.src_did.clone(),
                uri: l.subject_uri.clone(),
                cid: l.subject_cid.clone(),
                val: l.val.clone(),
                neg: l.neg,
                cts: l.created_at.clone(),
            })
            .collect(),
        cursor: cursor.map(String::from),
    };

    serde_json::to_string(&output).unwrap_or_else(|_| r#"{"labels":[]}"#.to_string())
}
