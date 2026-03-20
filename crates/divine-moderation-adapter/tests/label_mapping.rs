use divine_moderation_adapter::labels::{
    map_action_to_label, queue_inbound_moderation, ModerationAction, SubjectKind,
};
use divine_moderation_adapter::labels::vocabulary::{
    atproto_to_divine, divine_to_atproto, divine_to_nip32, VOCABULARY,
};

#[test]
fn label_mapping_maps_known_actions_to_divine_labels() {
    let action = ModerationAction {
        subject: SubjectKind::Post,
        subject_id: "at://did:plc:divine/app.bsky.feed.post/post-1".to_string(),
        action: "nsfw".to_string(),
        reason: Some("adult content".to_string()),
        inbound: false,
    };

    let label = map_action_to_label(&action).expect("known action should map");
    assert_eq!(label.value, "divine-adult");
}

#[test]
fn label_mapping_returns_none_for_unknown_actions() {
    let action = ModerationAction {
        subject: SubjectKind::Account,
        subject_id: "did:plc:divine-user".to_string(),
        action: "custom-internal-note".to_string(),
        reason: None,
        inbound: false,
    };

    assert!(map_action_to_label(&action).is_none());
}

#[test]
fn label_mapping_queues_inbound_actions_for_human_review() {
    let action = ModerationAction {
        subject: SubjectKind::Blob,
        subject_id: "bafkreidivineblob".to_string(),
        action: "spam".to_string(),
        reason: Some("reported by atproto".to_string()),
        inbound: true,
    };

    let queue_entry = queue_inbound_moderation(&action);
    assert_eq!(queue_entry.review_state, "pending-human-review");
    assert_eq!(queue_entry.subject_id, action.subject_id);
}

// ── Vocabulary tests ─────────────────────────────────────────────────

#[test]
fn vocabulary_covers_all_atproto_content_labels() {
    let atproto_labels = ["porn", "sexual", "nudity", "gore", "graphic-media", "self-harm"];
    for label in atproto_labels {
        assert!(
            atproto_to_divine(label).is_some(),
            "ATProto label '{}' should map to a divine label",
            label
        );
    }
}

#[test]
fn vocabulary_covers_all_atproto_system_labels() {
    assert_eq!(atproto_to_divine("!takedown"), Some("takedown"));
    assert_eq!(atproto_to_divine("!suspend"), Some("suspend"));
    assert_eq!(atproto_to_divine("!warn"), Some("content-warning"));
}

#[test]
fn divine_to_atproto_roundtrips_for_content_labels() {
    let divine_labels = ["nudity", "sexual", "porn", "graphic-media", "violence", "self-harm"];
    for label in divine_labels {
        let at_label = divine_to_atproto(label);
        assert!(at_label.is_some(), "Divine label '{}' should map to ATProto", label);
        let back = atproto_to_divine(at_label.unwrap());
        assert!(back.is_some(), "ATProto label should map back");
    }
}

#[test]
fn divine_to_nip32_maps_to_content_warning_namespace() {
    let (namespace, value) = divine_to_nip32("nudity").unwrap();
    assert_eq!(namespace, "content-warning");
    assert_eq!(value, "nudity");
}

#[test]
fn takedown_maps_to_nip09_not_nip32() {
    assert!(divine_to_nip32("takedown").is_none());
}
