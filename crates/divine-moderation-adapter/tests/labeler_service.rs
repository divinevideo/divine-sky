use divine_moderation_adapter::labels::labeler_service::{format_query_labels_response, QueryLabelsParams, StoredLabel};

#[test]
fn query_labels_formats_response_correctly() {
    let events = vec![
        StoredLabel {
            seq: 1,
            src_did: "did:plc:divine-labeler".to_string(),
            subject_uri: "at://did:plc:user1/app.bsky.feed.post/rkey1".to_string(),
            subject_cid: None,
            val: "nudity".to_string(),
            neg: false,
            created_at: "2026-03-20T12:00:00Z".to_string(),
        },
    ];
    let response = format_query_labels_response(&events, None);
    let json: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert!(json["labels"].is_array());
    assert_eq!(json["labels"][0]["val"], "nudity");
    assert_eq!(json["labels"][0]["src"], "did:plc:divine-labeler");
}

#[test]
fn query_labels_filters_by_uri_patterns() {
    let params = QueryLabelsParams {
        uri_patterns: Some(vec!["at://did:plc:user1/*".to_string()]),
        sources: None,
        limit: 50,
        cursor: None,
    };
    assert!(params.matches_uri("at://did:plc:user1/app.bsky.feed.post/rkey1"));
    assert!(!params.matches_uri("at://did:plc:user2/app.bsky.feed.post/rkey1"));
}
