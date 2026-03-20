use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Result};
use async_trait::async_trait;
use chrono::Utc;
use divine_atbridge::provisioner::{
    AccountLinkRecord, AccountLinkStore, AccountProvisioner, KeyPair, KeyStore, PdsAccountCreator,
    PendingAccountLink, PlcClient, PlcOperation, ProvisioningState,
};
use secp256k1::{PublicKey, Secp256k1, SecretKey};

#[derive(Clone, Default)]
struct SharedLinks {
    records: Arc<Mutex<HashMap<String, AccountLinkRecord>>>,
}

impl SharedLinks {
    fn insert(&self, record: AccountLinkRecord) {
        self.records
            .lock()
            .unwrap()
            .insert(record.nostr_pubkey.clone(), record);
    }

    fn get(&self, pubkey: &str) -> AccountLinkRecord {
        self.records
            .lock()
            .unwrap()
            .get(pubkey)
            .cloned()
            .expect("link must exist")
    }
}

struct LifecycleStore {
    links: SharedLinks,
}

#[async_trait]
impl AccountLinkStore for LifecycleStore {
    async fn get_link_by_pubkey(&self, nostr_pubkey: &str) -> Result<Option<AccountLinkRecord>> {
        Ok(self
            .links
            .records
            .lock()
            .unwrap()
            .get(nostr_pubkey)
            .cloned())
    }

    async fn get_link_by_handle(&self, handle: &str) -> Result<Option<AccountLinkRecord>> {
        Ok(self
            .links
            .records
            .lock()
            .unwrap()
            .values()
            .find(|record| record.handle == handle)
            .cloned())
    }

    async fn save_pending_link(
        &self,
        pending: PendingAccountLink<'_>,
    ) -> Result<AccountLinkRecord> {
        let now = Utc::now();
        let record = AccountLinkRecord {
            nostr_pubkey: pending.nostr_pubkey.to_string(),
            did: pending.did.map(ToOwned::to_owned),
            handle: pending.handle.to_string(),
            crosspost_enabled: pending.crosspost_enabled,
            signing_key_id: pending.signing_key_id.to_string(),
            plc_rotation_key_ref: pending.plc_rotation_key_ref.to_string(),
            provisioning_state: ProvisioningState::Pending,
            provisioning_error: None,
            disabled_at: None,
            created_at: now,
            updated_at: now,
        };
        self.links.insert(record.clone());
        Ok(record)
    }

    async fn mark_link_ready(&self, nostr_pubkey: &str, did: &str) -> Result<AccountLinkRecord> {
        let mut records = self.links.records.lock().unwrap();
        let record = records.get_mut(nostr_pubkey).expect("link must exist");
        record.did = Some(did.to_string());
        record.provisioning_state = ProvisioningState::Ready;
        record.provisioning_error = None;
        record.updated_at = Utc::now();
        Ok(record.clone())
    }

    async fn mark_link_failed(
        &self,
        nostr_pubkey: &str,
        did: Option<&str>,
        error: &str,
    ) -> Result<AccountLinkRecord> {
        let mut records = self.links.records.lock().unwrap();
        let record = records.get_mut(nostr_pubkey).expect("link must exist");
        record.did = did.map(ToOwned::to_owned).or_else(|| record.did.clone());
        record.provisioning_state = ProvisioningState::Failed;
        record.provisioning_error = Some(error.to_string());
        record.updated_at = Utc::now();
        Ok(record.clone())
    }
}

struct MockKeyStore {
    generated: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl KeyStore for MockKeyStore {
    async fn generate_keypair(&self, purpose: &str) -> Result<(String, KeyPair)> {
        let mut generated = self.generated.lock().unwrap();
        let id = format!("{purpose}-{}", generated.len() + 1);
        generated.push(id.clone());

        let secp = Secp256k1::new();
        let seed = [generated.len() as u8; 32];
        let secret = SecretKey::from_slice(&seed).unwrap();
        let public = PublicKey::from_secret_key(&secp, &secret);

        Ok((
            id,
            KeyPair {
                secret_key: secret,
                public_key: public,
            },
        ))
    }
}

struct MockPlcClient {
    calls: Arc<Mutex<Vec<PlcOperation>>>,
}

#[async_trait]
impl PlcClient for MockPlcClient {
    async fn create_did(&self, operation: &PlcOperation) -> Result<String> {
        self.calls.lock().unwrap().push(operation.clone());
        Ok("did:plc:testaccount".to_string())
    }
}

struct MockPdsCreator {
    fail: bool,
    calls: Arc<Mutex<Vec<(String, String)>>>,
}

#[async_trait]
impl PdsAccountCreator for MockPdsCreator {
    async fn create_account(&self, did: &str, handle: &str) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push((did.to_string(), handle.to_string()));
        if self.fail {
            bail!("PDS account creation failed");
        }
        Ok(())
    }
}

struct LifecycleHarness {
    provisioner: AccountProvisioner<MockKeyStore, MockPlcClient, MockPdsCreator, LifecycleStore>,
    generated: Arc<Mutex<Vec<String>>>,
    plc_calls: Arc<Mutex<Vec<PlcOperation>>>,
    pds_calls: Arc<Mutex<Vec<(String, String)>>>,
}

fn make_provisioner(links: SharedLinks, pds_fail: bool) -> LifecycleHarness {
    let generated = Arc::new(Mutex::new(Vec::new()));
    let plc_calls = Arc::new(Mutex::new(Vec::new()));
    let pds_calls = Arc::new(Mutex::new(Vec::new()));

    let provisioner = AccountProvisioner {
        key_store: MockKeyStore {
            generated: generated.clone(),
        },
        plc_client: MockPlcClient {
            calls: plc_calls.clone(),
        },
        pds_creator: MockPdsCreator {
            fail: pds_fail,
            calls: pds_calls.clone(),
        },
        link_store: LifecycleStore { links },
        pds_endpoint: "https://pds.divine.video".to_string(),
        handle_domain: "divine.video".to_string(),
    };

    LifecycleHarness {
        provisioner,
        generated,
        plc_calls,
        pds_calls,
    }
}

fn failed_record(pubkey: &str, handle: &str) -> AccountLinkRecord {
    let now = Utc::now();
    AccountLinkRecord {
        nostr_pubkey: pubkey.to_string(),
        did: Some("did:plc:retryme".to_string()),
        handle: handle.to_string(),
        crosspost_enabled: false,
        signing_key_id: "signing-key-1".to_string(),
        plc_rotation_key_ref: "plc-rotation-key-2".to_string(),
        provisioning_state: ProvisioningState::Failed,
        provisioning_error: Some("creating account on PDS failed".to_string()),
        disabled_at: None,
        created_at: now,
        updated_at: now,
    }
}

fn disabled_record(pubkey: &str, handle: &str) -> AccountLinkRecord {
    let now = Utc::now();
    AccountLinkRecord {
        nostr_pubkey: pubkey.to_string(),
        did: Some("did:plc:disabled".to_string()),
        handle: handle.to_string(),
        crosspost_enabled: false,
        signing_key_id: "signing-key-1".to_string(),
        plc_rotation_key_ref: "plc-rotation-key-2".to_string(),
        provisioning_state: ProvisioningState::Disabled,
        provisioning_error: None,
        disabled_at: Some(now),
        created_at: now,
        updated_at: now,
    }
}

fn pending_without_did_record(pubkey: &str, handle: &str) -> AccountLinkRecord {
    let now = Utc::now();
    AccountLinkRecord {
        nostr_pubkey: pubkey.to_string(),
        did: None,
        handle: handle.to_string(),
        crosspost_enabled: true,
        signing_key_id: "pending-signing:legacy".to_string(),
        plc_rotation_key_ref: "pending-rotation:legacy".to_string(),
        provisioning_state: ProvisioningState::Pending,
        provisioning_error: None,
        disabled_at: None,
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn provisioning_lifecycle_transitions_pending_to_ready_with_distinct_keys() {
    let links = SharedLinks::default();
    let harness = make_provisioner(links.clone(), false);

    let result = harness
        .provisioner
        .provision_account("npub_abc123", "alice.divine.video")
        .await
        .expect("provisioning should succeed");

    let stored = links.get("npub_abc123");
    assert_eq!(stored.provisioning_state, ProvisioningState::Ready);
    assert_eq!(stored.did.as_deref(), Some("did:plc:testaccount"));
    assert_ne!(stored.signing_key_id, stored.plc_rotation_key_ref);
    assert_eq!(harness.generated.lock().unwrap().len(), 2);
    assert_eq!(harness.plc_calls.lock().unwrap().len(), 1);
    assert_eq!(harness.pds_calls.lock().unwrap().len(), 1);
    assert_eq!(result.signing_key_id, stored.signing_key_id);
}

#[tokio::test]
async fn provisioning_lifecycle_retry_reuses_existing_failed_link() {
    let links = SharedLinks::default();
    links.insert(failed_record("npub_retry", "dana.divine.video"));

    let harness = make_provisioner(links.clone(), false);

    let result = harness
        .provisioner
        .provision_account("npub_retry", "dana.divine.video")
        .await
        .expect("retry should recover");

    let stored = links.get("npub_retry");
    assert_eq!(stored.provisioning_state, ProvisioningState::Ready);
    assert_eq!(stored.did.as_deref(), Some("did:plc:retryme"));
    assert!(harness.generated.lock().unwrap().is_empty());
    assert!(harness.plc_calls.lock().unwrap().is_empty());
    assert_eq!(harness.pds_calls.lock().unwrap().len(), 1);
    assert_eq!(result.did, "did:plc:retryme");
}

#[tokio::test]
async fn provisioning_lifecycle_rejects_disabled_link() {
    let links = SharedLinks::default();
    links.insert(disabled_record("npub_disabled", "erin.divine.video"));

    let harness = make_provisioner(links, false);

    let err = harness
        .provisioner
        .provision_account("npub_disabled", "erin.divine.video")
        .await
        .expect_err("disabled accounts should not be reprovisioned");

    assert!(err.to_string().contains("disabled"));
    assert!(harness.generated.lock().unwrap().is_empty());
    assert!(harness.plc_calls.lock().unwrap().is_empty());
    assert!(harness.pds_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn provisioning_lifecycle_pending_without_did_creates_fresh_identity() {
    let links = SharedLinks::default();
    links.insert(pending_without_did_record(
        "npub_pending",
        "zoe.divine.video",
    ));

    let harness = make_provisioner(links.clone(), false);

    let result = harness
        .provisioner
        .provision_account("npub_pending", "zoe.divine.video")
        .await
        .expect("pending rows without did should provision from scratch");

    let stored = links.get("npub_pending");
    assert_eq!(stored.provisioning_state, ProvisioningState::Ready);
    assert_eq!(stored.did.as_deref(), Some("did:plc:testaccount"));
    assert!(stored.crosspost_enabled, "opt-in flag should be preserved");
    assert_ne!(stored.signing_key_id, "pending-signing:legacy");
    assert_ne!(stored.plc_rotation_key_ref, "pending-rotation:legacy");
    assert_eq!(harness.generated.lock().unwrap().len(), 2);
    assert_eq!(harness.plc_calls.lock().unwrap().len(), 1);
    assert_eq!(harness.pds_calls.lock().unwrap().len(), 1);
    assert_eq!(result.did, "did:plc:testaccount");
}
