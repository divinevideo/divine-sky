use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use divine_atbridge::pds_host_backfill::{PdsHostBackfill, ReadyStateSync};
use divine_atbridge::plc_directory::PlcDirectoryClient;
use divine_atbridge::provisioner::{
    AccountLinkRecord, PlcOperation, PlcService, ProvisioningState,
};
use mockito::Matcher;
use serde_json::json;

#[derive(Clone, Default)]
struct RecordingSync {
    calls: Arc<Mutex<Vec<(String, String, String)>>>,
}

#[async_trait]
impl ReadyStateSync for RecordingSync {
    async fn sync_ready_state(&self, nostr_pubkey: &str, handle: &str, did: &str) -> Result<()> {
        self.calls.lock().unwrap().push((
            nostr_pubkey.to_string(),
            handle.to_string(),
            did.to_string(),
        ));
        Ok(())
    }
}

fn ready_account() -> AccountLinkRecord {
    let now = chrono::Utc::now();
    AccountLinkRecord {
        nostr_pubkey: "npub1alice".to_string(),
        did: Some("did:plc:alice123".to_string()),
        handle: "alice.divine.video".to_string(),
        crosspost_enabled: true,
        signing_key_id: "signing-key:alice".to_string(),
        plc_rotation_key_ref: "plc-rotation-key:alice".to_string(),
        provisioning_state: ProvisioningState::Ready,
        provisioning_error: None,
        disabled_at: None,
        created_at: now,
        updated_at: now,
    }
}

fn staging_operation() -> PlcOperation {
    let mut verification_methods = std::collections::BTreeMap::new();
    verification_methods.insert("atproto".to_string(), "did:key:zexample".to_string());

    let mut services = std::collections::BTreeMap::new();
    services.insert(
        "atproto_pds".to_string(),
        PlcService {
            service_type: "AtprotoPersonalDataServer".to_string(),
            endpoint: "https://pds.staging.dvines.org".to_string(),
        },
    );

    PlcOperation {
        op_type: "plc_operation".to_string(),
        rotation_keys: vec!["did:key:zrotation".to_string()],
        verification_methods,
        also_known_as: vec!["at://alice.divine.video".to_string()],
        services,
        prev: Some("bafyreicurrent".to_string()),
        sig: "sig".to_string(),
    }
}

#[tokio::test]
async fn pds_host_backfill_rewrites_staging_pds_and_keeps_ready_state() {
    let mut server = mockito::Server::new_async().await;
    let update_mock = server
        .mock("POST", "/did:plc:alice123")
        .match_body(Matcher::Json(json!({
            "type": "plc_operation",
            "rotationKeys": ["did:key:zrotation"],
            "verificationMethods": {
                "atproto": "did:key:zexample"
            },
            "alsoKnownAs": ["at://alice.divine.video"],
            "services": {
                "atproto_pds": {
                    "type": "AtprotoPersonalDataServer",
                    "endpoint": "https://pds.divine.video"
                }
            },
            "prev": "bafyreicurrent",
            "sig": "sig"
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create_async()
        .await;

    let sync = RecordingSync::default();
    let sync_calls = sync.calls.clone();

    let backfill = PdsHostBackfill::new(PlcDirectoryClient::new(server.url()), sync);
    let account = ready_account();

    backfill
        .backfill_ready_account(&account, staging_operation())
        .await
        .expect("backfill should succeed");

    update_mock.assert_async().await;
    let calls = sync_calls.lock().unwrap();
    assert_eq!(
        calls.as_slice(),
        [(
            "npub1alice".to_string(),
            "alice.divine.video".to_string(),
            "did:plc:alice123".to_string(),
        )]
    );
}
