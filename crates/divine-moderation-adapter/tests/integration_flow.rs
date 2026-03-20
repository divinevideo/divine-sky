//! End-to-end bidirectional moderation flow test (no DB, no network).

use divine_moderation_adapter::labels::outbound::OutboundLabel;
use divine_moderation_adapter::labels::inbound::{InboundAction, NostrAction, process_inbound_label, map_to_nostr_actions};
use divine_moderation_adapter::labels::vocabulary::atproto_to_divine;

#[test]
fn outbound_divine_label_roundtrips_through_atproto_and_back() {
    // Step 1: DiVine classifies a video as nudity
    let outbound = OutboundLabel::from_moderation_result(
        "sha256abc",
        "at://did:plc:user1/app.bsky.feed.post/rkey1",
        "QUARANTINE",
        &[("nudity", 0.91)],
        "did:plc:divine-labeler",
    );
    assert_eq!(outbound.len(), 1);
    let emitted_val = &outbound[0].val;

    // Step 2: That label comes back via subscribeLabels (simulated inbound)
    let divine_label = atproto_to_divine(emitted_val).unwrap();
    assert_eq!(divine_label, "nudity");

    // Step 3: Inbound processing decides what to do
    let action = process_inbound_label(
        "did:plc:divine-labeler",  // It's our own label coming back
        emitted_val,
        false,
        &["did:plc:divine-labeler"],
    );
    // Our own labels from ourselves are auto-approved
    assert_eq!(action, InboundAction::AutoApprove);
}

#[test]
fn external_takedown_flows_to_nostr_deletion() {
    // External labeler issues takedown
    let action = process_inbound_label(
        "did:plc:ozone",
        "!takedown",
        false,
        &["did:plc:ozone"],
    );
    assert_eq!(action, InboundAction::RequiresReview);

    // After human approval, map to Nostr action
    let nostr_actions = map_to_nostr_actions(
        "!takedown",
        false,
        "nostr-event-abc",
        "nostr-pubkey-xyz",
    );
    assert_eq!(nostr_actions.len(), 1);
    match &nostr_actions[0] {
        NostrAction::PublishDeletion { nostr_event_id, .. } => {
            assert_eq!(nostr_event_id, "nostr-event-abc");
        }
        other => panic!("Expected PublishDeletion, got {:?}", other),
    }
}

#[test]
fn external_content_label_flows_to_nip32() {
    // External labeler flags as sexual
    let action = process_inbound_label(
        "did:plc:ozone",
        "sexual",
        false,
        &["did:plc:ozone"],
    );
    assert_eq!(action, InboundAction::AutoApprove);

    let nostr_actions = map_to_nostr_actions(
        "sexual",
        false,
        "nostr-event-def",
        "nostr-pubkey-xyz",
    );
    match &nostr_actions[0] {
        NostrAction::PublishLabel { namespace, value, nostr_event_id } => {
            assert_eq!(namespace, "content-warning");
            assert_eq!(value, "sexual");
            assert_eq!(nostr_event_id, "nostr-event-def");
        }
        other => panic!("Expected PublishLabel, got {:?}", other),
    }
}
