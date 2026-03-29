use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use divine_atbridge::pds_host_backfill::{
    PdsHostBackfill, PlcMigrationSigner, ReadyStateSync,
};
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

#[derive(Clone)]
struct RecordingSigner {
    calls: Arc<Mutex<Vec<(String, String, String)>>>,
    signed_operation: PlcOperation,
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

#[async_trait]
impl PlcMigrationSigner for RecordingSigner {
    async fn sign_pds_migration(
        &self,
        account: &AccountLinkRecord,
        current_operation: &PlcOperation,
        target_pds_origin: &str,
    ) -> Result<PlcOperation> {
        self.calls.lock().unwrap().push((
            account.handle.clone(),
            current_operation
                .services
                .get("atproto_pds")
                .map(|service| service.endpoint.clone())
                .unwrap_or_default(),
            target_pds_origin.to_string(),
        ));
        Ok(self.signed_operation.clone())
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
    operation_with_endpoint("https://pds.staging.dvines.org", Some("bafyreicurrent"), "sig")
}

fn production_operation() -> PlcOperation {
    operation_with_endpoint("https://pds.divine.video", Some("bafyreiproduction"), "sig-production")
}

fn custom_operation(endpoint: &str) -> PlcOperation {
    operation_with_endpoint(endpoint, Some("bafyreicustom"), "sig-custom")
}

fn signed_successor_operation() -> PlcOperation {
    operation_with_endpoint(
        "https://pds.divine.video",
        Some("bafyreisuccessor"),
        "fresh-sig",
    )
}

fn operation_with_endpoint(endpoint: &str, prev: Option<&str>, sig: &str) -> PlcOperation {
    let mut verification_methods = std::collections::BTreeMap::new();
    verification_methods.insert("atproto".to_string(), "did:key:zexample".to_string());

    let mut services = std::collections::BTreeMap::new();
    services.insert(
        "atproto_pds".to_string(),
        PlcService {
            service_type: "AtprotoPersonalDataServer".to_string(),
            endpoint: endpoint.to_string(),
        },
    );

    PlcOperation {
        op_type: "plc_operation".to_string(),
        rotation_keys: vec!["did:key:zrotation".to_string()],
        verification_methods,
        also_known_as: vec!["at://alice.divine.video".to_string()],
        services,
        prev: prev.map(|value| value.to_string()),
        sig: sig.to_string(),
    }
}

#[tokio::test]
async fn pds_host_backfill_uses_signed_successor_and_keeps_ready_state() {
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
            "prev": "bafyreisuccessor",
            "sig": "fresh-sig"
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create_async()
        .await;

    let sync = RecordingSync::default();
    let sync_calls = sync.calls.clone();
    let signer_calls = Arc::new(Mutex::new(Vec::new()));
    let signer = RecordingSigner {
        calls: signer_calls.clone(),
        signed_operation: signed_successor_operation(),
    };

    let backfill = PdsHostBackfill::new(PlcDirectoryClient::new(server.url()), sync, signer);
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

    let signer_calls = signer_calls.lock().unwrap();
    assert_eq!(
        signer_calls.as_slice(),
        [(
            "alice.divine.video".to_string(),
            "https://pds.staging.dvines.org".to_string(),
            "https://pds.divine.video".to_string(),
        )]
    );
}

#[tokio::test]
async fn pds_host_backfill_skips_plc_update_when_account_is_already_on_production() {
    let sync = RecordingSync::default();
    let sync_calls = sync.calls.clone();
    let signer_calls = Arc::new(Mutex::new(Vec::new()));
    let signer = RecordingSigner {
        calls: signer_calls.clone(),
        signed_operation: signed_successor_operation(),
    };

    let backfill = PdsHostBackfill::new(
        PlcDirectoryClient::new("http://127.0.0.1:9"),
        sync,
        signer,
    );
    let account = ready_account();

    backfill
        .backfill_ready_account(&account, production_operation())
        .await
        .expect("already-migrated accounts should only refresh ready state");

    let calls = sync_calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert!(signer_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn pds_host_backfill_rejects_non_staging_hosts() {
    let sync = RecordingSync::default();
    let sync_calls = sync.calls.clone();
    let signer_calls = Arc::new(Mutex::new(Vec::new()));
    let signer = RecordingSigner {
        calls: signer_calls.clone(),
        signed_operation: signed_successor_operation(),
    };

    let backfill = PdsHostBackfill::new(
        PlcDirectoryClient::new("http://127.0.0.1:9"),
        sync,
        signer,
    );
    let account = ready_account();

    let error = backfill
        .backfill_ready_account(&account, custom_operation("https://pds.other.example"))
        .await
        .expect_err("unexpected hosts should not be rewritten by the backfill helper");

    assert!(error
        .to_string()
        .contains("backfill only supports the legacy staging PDS host"));
    assert!(sync_calls.lock().unwrap().is_empty());
    assert!(signer_calls.lock().unwrap().is_empty());
}
