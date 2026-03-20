use chrono::Utc;
use divine_bridge_db::models::LabelerEvent;
use divine_labeler::routes::query_labels::build_query_response;

#[test]
fn build_query_response_formats_labels_correctly() {
    let events = vec![LabelerEvent {
        seq: 1,
        src_did: "did:plc:test-labeler".to_string(),
        subject_uri: "at://did:plc:user1/app.bsky.feed.post/rkey1".to_string(),
        subject_cid: None,
        val: "nudity".to_string(),
        neg: false,
        nostr_event_id: None,
        sha256: Some("abc123".to_string()),
        origin: "divine".to_string(),
        created_at: Utc::now(),
    }];

    let (body, cursor) = build_query_response(&events);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(json["labels"].is_array());
    assert_eq!(json["labels"][0]["val"], "nudity");
    assert_eq!(json["labels"][0]["src"], "did:plc:test-labeler");
    assert_eq!(json["labels"][0]["ver"], 1);
    assert!(cursor.is_some());
}

#[test]
fn build_query_response_empty_events_returns_empty_labels() {
    let (body, cursor) = build_query_response(&[]);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["labels"].as_array().unwrap().len(), 0);
    assert!(cursor.is_none());
}
