use chrono::Utc;
use divine_bridge_db::models::{AccountLinkLifecycleRow, PublishJob};
use divine_bridge_types::PublishJobSource;
use serde_json::json;

#[test]
fn publish_job_rows_include_scheduler_metadata() {
    let now = Utc::now();
    let job = PublishJob {
        nostr_event_id: "event-1".to_string(),
        nostr_pubkey: "npub1alice".to_string(),
        event_created_at: now,
        event_payload: json!({
            "id": "event-1",
            "pubkey": "npub1alice",
            "created_at": 1700000000,
            "kind": 34235,
            "tags": [],
            "content": "hello",
            "sig": "sig"
        }),
        job_source: PublishJobSource::Backfill.as_str().to_string(),
        attempt: 0,
        state: "pending".to_string(),
        error: None,
        lease_owner: None,
        lease_expires_at: None,
        completed_at: None,
        created_at: now,
        updated_at: now,
    };

    assert_eq!(job.nostr_pubkey, "npub1alice");
    assert_eq!(job.job_source, PublishJobSource::Backfill.as_str());
    assert_eq!(job.event_payload["id"], "event-1");
}

#[test]
fn account_link_rows_include_backfill_state() {
    let now = Utc::now();
    let row = AccountLinkLifecycleRow {
        nostr_pubkey: "npub1alice".to_string(),
        did: Some("did:plc:alice".to_string()),
        handle: "alice.divine.video".to_string(),
        crosspost_enabled: true,
        signing_key_id: Some("signing-key-1".to_string()),
        plc_rotation_key_ref: Some("rotation-key-1".to_string()),
        provisioning_state: "ready".to_string(),
        provisioning_error: None,
        publish_backfill_state: "not_started".to_string(),
        publish_backfill_started_at: None,
        publish_backfill_completed_at: None,
        publish_backfill_error: None,
        disabled_at: None,
        created_at: now,
        updated_at: now,
    };

    assert_eq!(row.publish_backfill_state, "not_started");
    assert!(row.publish_backfill_started_at.is_none());
}
