//! Inbound label processing: ATProto labels → DiVine moderation queue.

use super::vocabulary::{atproto_to_divine, divine_to_nip32, get_entry_by_atproto};

/// What to do with an inbound label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundAction {
    AutoApprove,
    RequiresReview,
    Ignore,
}

/// Determine the action for an inbound ATProto label.
pub fn process_inbound_label(
    labeler_did: &str,
    atproto_val: &str,
    neg: bool,
    trusted_labelers: &[&str],
) -> InboundAction {
    if atproto_to_divine(atproto_val).is_none() {
        return InboundAction::Ignore;
    }

    let is_trusted = trusted_labelers.contains(&labeler_did);

    // Enforcement labels ALWAYS require human review (unless negation)
    let enforcement = get_entry_by_atproto(atproto_val)
        .map(|e| e.requires_enforcement)
        .unwrap_or(false);

    if enforcement && !neg {
        return InboundAction::RequiresReview;
    }

    if is_trusted {
        return InboundAction::AutoApprove;
    }

    InboundAction::RequiresReview
}

/// What Nostr action(s) to take for an approved inbound label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NostrAction {
    PublishLabel {
        namespace: String,
        value: String,
        nostr_event_id: String,
    },
    PublishDeletion {
        nostr_event_id: String,
        reason: String,
    },
    RelayBan {
        nostr_pubkey: String,
        reason: String,
    },
    None,
}

/// Map an approved inbound label to Nostr action(s).
pub fn map_to_nostr_actions(
    atproto_val: &str,
    neg: bool,
    nostr_event_id: &str,
    nostr_pubkey: &str,
) -> Vec<NostrAction> {
    let divine_label = match atproto_to_divine(atproto_val) {
        Some(l) => l,
        None => return vec![NostrAction::None],
    };

    match atproto_val {
        "!takedown" if !neg => vec![NostrAction::PublishDeletion {
            nostr_event_id: nostr_event_id.to_string(),
            reason: "ATProto takedown label from labeler".to_string(),
        }],
        "!suspend" if !neg => vec![NostrAction::RelayBan {
            nostr_pubkey: nostr_pubkey.to_string(),
            reason: "ATProto account suspension".to_string(),
        }],
        _ => {
            if let Some((namespace, value)) = divine_to_nip32(divine_label) {
                vec![NostrAction::PublishLabel {
                    namespace: namespace.to_string(),
                    value: if neg {
                        format!("not-{}", value)
                    } else {
                        value.to_string()
                    },
                    nostr_event_id: nostr_event_id.to_string(),
                }]
            } else {
                vec![NostrAction::None]
            }
        }
    }
}
