use divine_atbridge::profile_sync::{
    build_profile_record, parse_kind0_profile, PROFILE_COLLECTION,
};
use divine_bridge_types::NostrEvent;

fn kind0_event(content: &str) -> NostrEvent {
    NostrEvent {
        id: "profile-event".to_string(),
        pubkey: "pubkey".to_string(),
        created_at: 1_700_000_000,
        kind: 0,
        tags: vec![],
        content: content.to_string(),
        sig: "sig".to_string(),
    }
}

#[test]
fn profile_record_uses_website_field_and_created_at() {
    let parsed = parse_kind0_profile(&kind0_event(
        r#"{
            "display_name":"DiVine",
            "about":"Short bio",
            "website":"https://divine.video"
        }"#,
    ))
    .unwrap();

    let record = build_profile_record(&parsed, None, None);

    assert_eq!(record["$type"], PROFILE_COLLECTION);
    assert_eq!(record["displayName"], "DiVine");
    assert_eq!(record["description"], "Short bio");
    assert_eq!(record["website"], "https://divine.video");
    assert_eq!(record["createdAt"], "2023-11-14T22:13:20Z");
}
