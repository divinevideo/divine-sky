use divine_moderation_adapter::labels::inbound::{process_inbound_label, InboundAction};

#[test]
fn content_label_auto_approved_for_trusted_labeler() {
    let action =
        process_inbound_label("did:plc:ozone-mod", "sexual", false, &["did:plc:ozone-mod"]);
    assert_eq!(action, InboundAction::AutoApprove);
}

#[test]
fn takedown_always_requires_review() {
    let action = process_inbound_label(
        "did:plc:ozone-mod",
        "!takedown",
        false,
        &["did:plc:ozone-mod"],
    );
    assert_eq!(action, InboundAction::RequiresReview);
}

#[test]
fn untrusted_labeler_requires_review() {
    let action = process_inbound_label(
        "did:plc:random-labeler",
        "nudity",
        false,
        &["did:plc:ozone-mod"],
    );
    assert_eq!(action, InboundAction::RequiresReview);
}

#[test]
fn negation_from_trusted_labeler_auto_approved() {
    let action = process_inbound_label("did:plc:ozone-mod", "nudity", true, &["did:plc:ozone-mod"]);
    assert_eq!(action, InboundAction::AutoApprove);
}
