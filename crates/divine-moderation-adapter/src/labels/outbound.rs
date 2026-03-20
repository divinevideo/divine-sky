//! Outbound label emission: DiVine moderation results → ATProto label records.

use super::vocabulary::divine_to_atproto;

/// An outbound label ready to be inserted into labeler_events.
#[derive(Debug, Clone)]
pub struct OutboundLabel {
    pub subject_uri: String,
    pub sha256: String,
    pub val: String,
    pub neg: bool,
    pub src_did: String,
}

/// Score threshold for emitting a content label.
const LABEL_CONFIDENCE_THRESHOLD: f64 = 0.5;

impl OutboundLabel {
    /// Generate ATProto labels from a moderation result.
    ///
    /// `scores` is a slice of (divine_category, score) pairs.
    /// Only scores above threshold produce labels.
    /// PERMANENT_BAN also produces a `!takedown` system label.
    pub fn from_moderation_result(
        sha256: &str,
        at_uri: &str,
        action: &str,
        scores: &[(&str, f64)],
        labeler_did: &str,
    ) -> Vec<Self> {
        if action == "SAFE" {
            return vec![];
        }

        let mut labels = Vec::new();

        for (category, score) in scores {
            if *score < LABEL_CONFIDENCE_THRESHOLD {
                continue;
            }
            if let Some(at_val) = divine_to_atproto(category) {
                labels.push(Self {
                    subject_uri: at_uri.to_string(),
                    sha256: sha256.to_string(),
                    val: at_val.to_string(),
                    neg: false,
                    src_did: labeler_did.to_string(),
                });
            }
        }

        if action == "PERMANENT_BAN" {
            labels.push(Self {
                subject_uri: at_uri.to_string(),
                sha256: sha256.to_string(),
                val: "!takedown".to_string(),
                neg: false,
                src_did: labeler_did.to_string(),
            });
        }

        labels
    }

    /// Generate a negation label (human moderator rejected a category).
    pub fn from_rejection(
        sha256: &str,
        at_uri: &str,
        divine_category: &str,
        labeler_did: &str,
    ) -> Vec<Self> {
        if let Some(at_val) = divine_to_atproto(divine_category) {
            vec![Self {
                subject_uri: at_uri.to_string(),
                sha256: sha256.to_string(),
                val: at_val.to_string(),
                neg: true,
                src_did: labeler_did.to_string(),
            }]
        } else {
            vec![]
        }
    }
}
