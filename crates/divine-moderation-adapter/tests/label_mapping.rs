#[path = "../src/labels.rs"]
mod labels;

use labels::{map_action_to_label, queue_inbound_moderation, ModerationAction, SubjectKind};

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
