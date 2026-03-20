use chrono::Utc;
use divine_atbridge::runtime::account_link_from_lifecycle_row;
use divine_bridge_db::models::AccountLinkLifecycleRow;

fn lifecycle_row(crosspost_enabled: bool, provisioning_state: &str) -> AccountLinkLifecycleRow {
    let now = Utc::now();
    AccountLinkLifecycleRow {
        nostr_pubkey: "npub_gate".to_string(),
        did: Some("did:plc:gate".to_string()),
        handle: "gate.divine.video".to_string(),
        crosspost_enabled,
        signing_key_id: Some("signing-key-1".to_string()),
        plc_rotation_key_ref: Some("rotation-key-1".to_string()),
        provisioning_state: provisioning_state.to_string(),
        provisioning_error: None,
        disabled_at: None,
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn ready_but_not_crosspost_enabled_is_skipped() {
    let row = lifecycle_row(false, "ready");
    let mapped = account_link_from_lifecycle_row(&row);
    assert!(mapped.is_none());
}

#[test]
fn ready_and_crosspost_enabled_is_publishable() {
    let row = lifecycle_row(true, "ready");
    let mapped = account_link_from_lifecycle_row(&row).expect("ready + enabled should pass");
    assert_eq!(mapped.did, "did:plc:gate");
    assert!(mapped.opted_in);
}
