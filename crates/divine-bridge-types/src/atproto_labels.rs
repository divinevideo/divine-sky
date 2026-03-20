//! ATProto label record types for the labeler service and inbound subscriber.
//!
//! These types implement the `com.atproto.label` lexicon, used for publishing
//! and subscribing to content labels via the ATProto firehose.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Core label record
// ---------------------------------------------------------------------------

/// An ATProto label record as defined by the `com.atproto.label#label` lexicon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtprotoLabel {
    /// Label format version (currently 1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ver: Option<u32>,

    /// DID of the labeler that created this label.
    pub src: String,

    /// AT URI of the subject being labeled (e.g. `at://did:plc:.../collection/rkey`).
    pub uri: String,

    /// Optional CID of the specific record version being labeled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,

    /// The label value (e.g. `"sexual"`, `"nudity"`, `"!hide"`).
    pub val: String,

    /// When true, this label negates a previously applied label with the same `val`.
    pub neg: bool,

    /// ISO 8601 creation timestamp, e.g. `"2026-03-20T12:00:00.000Z"`.
    pub cts: String,

    /// Optional ISO 8601 expiry timestamp after which the label is no longer valid.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<String>,

    /// Optional base64-encoded cryptographic signature over the label record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
}

impl AtprotoLabel {
    /// Returns true if this is a system/administrative label (value starts with `!`).
    pub fn is_system_label(&self) -> bool {
        self.val.starts_with('!')
    }

    /// Returns true if this label targets an app.bsky.feed.post record.
    pub fn targets_post(&self) -> bool {
        self.uri.contains("/app.bsky.feed.post/")
    }

    /// Returns true if this label targets an account (URI is a bare DID with no path).
    pub fn targets_account(&self) -> bool {
        self.uri.starts_with("did:") && !self.uri.contains('/')
    }

    /// Extract the DID from the subject URI.
    ///
    /// For AT URIs (`at://did:plc:xxx/...`) returns the authority portion.
    /// For bare DIDs returns the URI itself.
    /// Returns `None` if the URI does not appear to contain a DID.
    pub fn subject_did(&self) -> Option<&str> {
        if let Some(rest) = self.uri.strip_prefix("at://") {
            // AT URI: authority is everything up to the next '/'
            let authority = rest.split('/').next().unwrap_or(rest);
            if authority.starts_with("did:") {
                return Some(authority);
            }
        } else if self.uri.starts_with("did:") {
            return Some(&self.uri);
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Subscription message envelope
// ---------------------------------------------------------------------------

/// Messages emitted by a `com.atproto.label.subscribeLabels` event stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SubscribeLabelsMessage {
    /// A batch of labels at the given sequence number.
    Labels { seq: i64, labels: Vec<AtprotoLabel> },
    /// An informational/control message from the labeler.
    Info {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_serializes_to_atproto_format() {
        let label = AtprotoLabel {
            ver: Some(1),
            src: "did:plc:divine-labeler".to_string(),
            uri: "at://did:plc:user123/app.bsky.feed.post/abc123".to_string(),
            cid: None,
            val: "sexual".to_string(),
            neg: false,
            cts: "2026-03-20T12:00:00.000Z".to_string(),
            exp: None,
            sig: None,
        };
        let json = serde_json::to_value(&label).unwrap();
        assert_eq!(json["ver"], 1);
        assert_eq!(json["src"], "did:plc:divine-labeler");
        assert_eq!(json["val"], "sexual");
        assert_eq!(json["neg"], false);
        assert!(json.get("cid").is_none());
    }

    #[test]
    fn negation_label_round_trips() {
        let label = AtprotoLabel {
            ver: Some(1),
            src: "did:plc:divine-labeler".to_string(),
            uri: "at://did:plc:user123/app.bsky.feed.post/abc123".to_string(),
            cid: None,
            val: "nudity".to_string(),
            neg: true,
            cts: "2026-03-20T12:00:00.000Z".to_string(),
            exp: None,
            sig: None,
        };
        let json_str = serde_json::to_string(&label).unwrap();
        let back: AtprotoLabel = serde_json::from_str(&json_str).unwrap();
        assert!(back.neg);
        assert_eq!(back.val, "nudity");
    }

    #[test]
    fn subscribe_labels_message_parses_labels_variant() {
        let json = r#"{"seq":42,"labels":[{"ver":1,"src":"did:plc:ozone","uri":"at://did:plc:u/app.bsky.feed.post/x","val":"porn","neg":false,"cts":"2026-03-20T00:00:00Z"}]}"#;
        let msg: SubscribeLabelsMessage = serde_json::from_str(json).unwrap();
        match msg {
            SubscribeLabelsMessage::Labels { seq, labels } => {
                assert_eq!(seq, 42);
                assert_eq!(labels.len(), 1);
                assert_eq!(labels[0].val, "porn");
            }
            _ => panic!("Expected Labels variant"),
        }
    }

    #[test]
    fn is_system_label_detects_bang_prefix() {
        let mut label = AtprotoLabel {
            ver: Some(1),
            src: "did:plc:x".to_string(),
            uri: "at://did:plc:x".to_string(),
            cid: None,
            val: "!hide".to_string(),
            neg: false,
            cts: "2026-03-20T00:00:00Z".to_string(),
            exp: None,
            sig: None,
        };
        assert!(label.is_system_label());
        label.val = "nudity".to_string();
        assert!(!label.is_system_label());
    }

    #[test]
    fn targets_post_and_account() {
        let post_label = AtprotoLabel {
            ver: None,
            src: "did:plc:x".to_string(),
            uri: "at://did:plc:user/app.bsky.feed.post/rkey".to_string(),
            cid: None,
            val: "porn".to_string(),
            neg: false,
            cts: "2026-03-20T00:00:00Z".to_string(),
            exp: None,
            sig: None,
        };
        assert!(post_label.targets_post());
        assert!(!post_label.targets_account());

        let account_label = AtprotoLabel {
            uri: "did:plc:user123".to_string(),
            ..post_label
        };
        assert!(!account_label.targets_post());
        assert!(account_label.targets_account());
    }

    #[test]
    fn subject_did_extracts_from_at_uri() {
        let label = AtprotoLabel {
            ver: None,
            src: "did:plc:x".to_string(),
            uri: "at://did:plc:user123/app.bsky.feed.post/rkey".to_string(),
            cid: None,
            val: "porn".to_string(),
            neg: false,
            cts: "2026-03-20T00:00:00Z".to_string(),
            exp: None,
            sig: None,
        };
        assert_eq!(label.subject_did(), Some("did:plc:user123"));

        let bare = AtprotoLabel {
            uri: "did:plc:user123".to_string(),
            ..label
        };
        assert_eq!(bare.subject_did(), Some("did:plc:user123"));
    }
}
