use divine_moderation_adapter::labels::outbound::OutboundLabel;

#[test]
fn quarantine_nudity_produces_atproto_label() {
    let result = OutboundLabel::from_moderation_result(
        "abc123sha256",
        "at://did:plc:user1/app.bsky.feed.post/rkey1",
        "QUARANTINE",
        &[("nudity", 0.91)],
        "did:plc:divine-labeler",
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].val, "nudity");
    assert_eq!(
        result[0].subject_uri,
        "at://did:plc:user1/app.bsky.feed.post/rkey1"
    );
    assert!(!result[0].neg);
}

#[test]
fn safe_result_produces_no_labels() {
    let result = OutboundLabel::from_moderation_result(
        "abc123sha256",
        "at://did:plc:user1/app.bsky.feed.post/rkey1",
        "SAFE",
        &[("nudity", 0.1)],
        "did:plc:divine-labeler",
    );
    assert!(result.is_empty());
}

#[test]
fn permanent_ban_produces_takedown_label() {
    let result = OutboundLabel::from_moderation_result(
        "abc123sha256",
        "at://did:plc:user1/app.bsky.feed.post/rkey1",
        "PERMANENT_BAN",
        &[("violence", 0.95)],
        "did:plc:divine-labeler",
    );
    let vals: Vec<&str> = result.iter().map(|l| l.val.as_str()).collect();
    assert!(vals.contains(&"violence"));
    assert!(vals.contains(&"!takedown"));
}

#[test]
fn negation_label_for_human_rejection() {
    let result = OutboundLabel::from_rejection(
        "abc123sha256",
        "at://did:plc:user1/app.bsky.feed.post/rkey1",
        "nudity",
        "did:plc:divine-labeler",
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].val, "nudity");
    assert!(result[0].neg);
}
