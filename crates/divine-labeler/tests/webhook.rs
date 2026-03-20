use divine_labeler::routes::webhook::WebhookPayload;

#[test]
fn webhook_payload_deserializes_from_js_format() {
    let json = r#"{
        "sha256": "abc123",
        "action": "QUARANTINE",
        "labels": [
            {"category": "nudity", "score": 0.91}
        ],
        "reviewed_by": null,
        "timestamp": "2026-03-20T12:00:00.000Z",
        "nostr_event_id": null
    }"#;

    let payload: WebhookPayload = serde_json::from_str(json).unwrap();
    assert_eq!(payload.sha256, "abc123");
    assert_eq!(payload.action, "QUARANTINE");
    assert_eq!(payload.labels.len(), 1);
    assert_eq!(payload.labels[0].category, "nudity");
}

#[test]
fn webhook_payload_handles_multiple_labels() {
    let json = r#"{
        "sha256": "def456",
        "action": "PERMANENT_BAN",
        "labels": [
            {"category": "violence", "score": 0.95},
            {"category": "gore", "score": 0.88}
        ],
        "reviewed_by": "admin",
        "timestamp": "2026-03-20T12:00:00.000Z",
        "nostr_event_id": "abc123eventid"
    }"#;

    let payload: WebhookPayload = serde_json::from_str(json).unwrap();
    assert_eq!(payload.labels.len(), 2);
    assert_eq!(payload.reviewed_by, Some("admin".to_string()));
    assert_eq!(payload.nostr_event_id, Some("abc123eventid".to_string()));
}

#[test]
fn webhook_payload_handles_empty_labels() {
    let json = r#"{
        "sha256": "ghi789",
        "action": "REVIEW",
        "labels": [],
        "reviewed_by": null,
        "timestamp": "2026-03-20T12:00:00.000Z",
        "nostr_event_id": null
    }"#;

    let payload: WebhookPayload = serde_json::from_str(json).unwrap();
    assert!(payload.labels.is_empty());
}
