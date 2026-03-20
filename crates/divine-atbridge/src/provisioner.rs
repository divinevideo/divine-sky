//! Account provisioning for linking Nostr users to ATProto accounts.

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use secp256k1::{PublicKey, Secp256k1, SecretKey};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Result of successfully provisioning or recovering an ATProto account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionResult {
    pub did: String,
    pub handle: String,
    pub signing_key_id: String,
}

/// A generated secp256k1 keypair.
#[derive(Debug, Clone)]
pub struct KeyPair {
    pub secret_key: SecretKey,
    pub public_key: PublicKey,
}

/// Lifecycle states for a linked account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvisioningState {
    Pending,
    Ready,
    Failed,
    Disabled,
}

impl ProvisioningState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Disabled => "disabled",
        }
    }
}

/// Durable account-link state used by the control plane and provisioner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountLinkRecord {
    pub nostr_pubkey: String,
    pub did: Option<String>,
    pub handle: String,
    pub crosspost_enabled: bool,
    pub signing_key_id: String,
    pub plc_rotation_key_ref: String,
    pub provisioning_state: ProvisioningState,
    pub provisioning_error: Option<String>,
    pub disabled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Input used when first persisting an in-progress account link.
#[derive(Debug, Clone, Copy)]
pub struct PendingAccountLink<'a> {
    pub nostr_pubkey: &'a str,
    pub did: Option<&'a str>,
    pub handle: &'a str,
    pub crosspost_enabled: bool,
    pub signing_key_id: &'a str,
    pub plc_rotation_key_ref: &'a str,
}

/// PLC operation sent to the PLC directory.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlcOperation {
    #[serde(rename = "type")]
    pub op_type: String,
    pub rotation_keys: Vec<String>,
    pub verification_methods: std::collections::BTreeMap<String, String>,
    pub also_known_as: Vec<String>,
    pub services: std::collections::BTreeMap<String, PlcService>,
    pub prev: Option<String>,
    pub sig: String,
}

/// A service entry in a PLC operation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlcService {
    #[serde(rename = "type")]
    pub service_type: String,
    pub endpoint: String,
}

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Generates and stores provisioning keys.
#[async_trait]
pub trait KeyStore: Send + Sync {
    async fn generate_keypair(&self, purpose: &str) -> Result<(String, KeyPair)>;
}

/// Interacts with the PLC directory to create DIDs.
#[async_trait]
pub trait PlcClient: Send + Sync {
    async fn create_did(&self, operation: &PlcOperation) -> Result<String>;
}

/// Creates accounts on the PDS.
#[async_trait]
pub trait PdsAccountCreator: Send + Sync {
    async fn create_account(&self, did: &str, handle: &str) -> Result<()>;
}

/// Stores lifecycle-aware account-link state.
#[async_trait]
pub trait AccountLinkStore: Send + Sync {
    async fn get_link_by_pubkey(&self, nostr_pubkey: &str) -> Result<Option<AccountLinkRecord>>;
    async fn get_link_by_handle(&self, handle: &str) -> Result<Option<AccountLinkRecord>>;
    async fn save_pending_link(&self, pending: PendingAccountLink<'_>) -> Result<AccountLinkRecord>;
    async fn mark_link_ready(&self, nostr_pubkey: &str, did: &str) -> Result<AccountLinkRecord>;
    async fn mark_link_failed(
        &self,
        nostr_pubkey: &str,
        did: Option<&str>,
        error: &str,
    ) -> Result<AccountLinkRecord>;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Encode a secp256k1 compressed public key as `did:key:z...`.
pub fn pubkey_to_did_key(pubkey: &PublicKey) -> String {
    let compressed = pubkey.serialize();
    let mut buf = vec![0xe7u8, 0x01];
    buf.extend_from_slice(&compressed);
    let encoded = bs58::encode(&buf).into_string();
    format!("did:key:z{encoded}")
}

fn unsigned_operation_bytes(operation: &PlcOperation) -> Vec<u8> {
    let mut value = serde_json::to_value(operation).expect("PlcOperation serialises to JSON");
    if let Some(obj) = value.as_object_mut() {
        obj.remove("sig");
    }
    serde_ipld_dagcbor::to_vec(&value).expect("valid PLC operations encode to DAG-CBOR")
}

pub fn derive_did_plc(operation: &PlcOperation) -> String {
    use sha2::{Digest, Sha256};

    let cbor_bytes = unsigned_operation_bytes(operation);
    let hash = Sha256::digest(&cbor_bytes);
    let encoded = data_encoding::BASE32_NOPAD
        .encode(&hash[..15])
        .to_ascii_lowercase();
    format!("did:plc:{encoded}")
}

pub fn sign_plc_operation(operation: &PlcOperation, secret_key: &SecretKey) -> Result<String> {
    use sha2::{Digest, Sha256};

    let cbor_bytes = unsigned_operation_bytes(operation);
    let hash: [u8; 32] = Sha256::digest(&cbor_bytes).into();
    let msg = secp256k1::Message::from_digest(hash);
    let secp = Secp256k1::new();
    let sig = secp.sign_ecdsa(&msg, secret_key);
    Ok(data_encoding::BASE64URL_NOPAD.encode(&sig.serialize_compact()))
}

// ---------------------------------------------------------------------------
// Provisioner
// ---------------------------------------------------------------------------

pub struct AccountProvisioner<K, P, A, L>
where
    K: KeyStore,
    P: PlcClient,
    A: PdsAccountCreator,
    L: AccountLinkStore,
{
    pub key_store: K,
    pub plc_client: P,
    pub pds_creator: A,
    pub link_store: L,
    pub pds_endpoint: String,
    pub handle_domain: String,
}

impl<K, P, A, L> AccountProvisioner<K, P, A, L>
where
    K: KeyStore,
    P: PlcClient,
    A: PdsAccountCreator,
    L: AccountLinkStore,
{
    pub async fn provision_account(
        &self,
        nostr_pubkey: &str,
        handle: &str,
    ) -> Result<ProvisionResult> {
        let normalised_handle = self.normalise_handle(handle);
        self.validate_handle(&normalised_handle)?;

        if let Some(existing) = self
            .link_store
            .get_link_by_pubkey(nostr_pubkey)
            .await
            .context("looking up link by pubkey")?
        {
            return self
                .resume_or_reject(nostr_pubkey, &normalised_handle, existing)
                .await;
        }

        if let Some(existing) = self
            .link_store
            .get_link_by_handle(&normalised_handle)
            .await
            .context("looking up link by handle")?
        {
            if existing.nostr_pubkey != nostr_pubkey {
                bail!("handle already taken: {normalised_handle}");
            }

            return self
                .resume_or_reject(nostr_pubkey, &normalised_handle, existing)
                .await;
        }

        self.create_new_link(nostr_pubkey, &normalised_handle).await
    }

    fn normalise_handle(&self, handle: &str) -> String {
        handle.trim().to_ascii_lowercase()
    }

    fn validate_handle(&self, handle: &str) -> Result<()> {
        let domain = self.handle_domain.trim().trim_start_matches('.');
        let expected_suffix = format!(".{domain}");
        if !handle.ends_with(&expected_suffix) {
            bail!("handle must end with {expected_suffix}");
        }
        if handle == domain {
            bail!("handle must include a username");
        }
        Ok(())
    }

    async fn resume_or_reject(
        &self,
        nostr_pubkey: &str,
        handle: &str,
        existing: AccountLinkRecord,
    ) -> Result<ProvisionResult> {
        if existing.handle != handle {
            bail!("existing link is bound to a different handle");
        }

        match existing.provisioning_state {
            ProvisioningState::Disabled => bail!("account link is disabled"),
            ProvisioningState::Ready => {
                let did = existing
                    .did
                    .clone()
                    .context("ready link is missing a DID")?;
                Ok(ProvisionResult {
                    did,
                    handle: existing.handle,
                    signing_key_id: existing.signing_key_id,
                })
            }
            ProvisioningState::Pending | ProvisioningState::Failed => {
                let did = existing
                    .did
                    .clone()
                    .context("cannot retry a lifecycle record without a DID")?;
                let record = self
                    .create_pds_account_and_mark_ready(nostr_pubkey, &did, handle)
                    .await?;
                Ok(ProvisionResult {
                    did,
                    handle: record.handle,
                    signing_key_id: record.signing_key_id,
                })
            }
        }
    }

    async fn create_new_link(
        &self,
        nostr_pubkey: &str,
        handle: &str,
    ) -> Result<ProvisionResult> {
        let (signing_key_id, signing_keypair) = self
            .key_store
            .generate_keypair("signing-key")
            .await
            .context("generating AT signing keypair")?;
        let (rotation_key_id, rotation_keypair) = self
            .key_store
            .generate_keypair("plc-rotation-key")
            .await
            .context("generating PLC rotation keypair")?;

        self.link_store
            .save_pending_link(PendingAccountLink {
                nostr_pubkey,
                did: None,
                handle,
                crosspost_enabled: false,
                signing_key_id: &signing_key_id,
                plc_rotation_key_ref: &rotation_key_id,
            })
            .await
            .context("saving pending account link")?;

        let signing_did_key = pubkey_to_did_key(&signing_keypair.public_key);
        let rotation_did_key = pubkey_to_did_key(&rotation_keypair.public_key);

        let mut verification_methods = std::collections::BTreeMap::new();
        verification_methods.insert("atproto".to_string(), signing_did_key);

        let mut services = std::collections::BTreeMap::new();
        services.insert(
            "atproto_pds".to_string(),
            PlcService {
                service_type: "AtprotoPersonalDataServer".to_string(),
                endpoint: self.pds_endpoint.clone(),
            },
        );

        let mut operation = PlcOperation {
            op_type: "plc_operation".to_string(),
            rotation_keys: vec![rotation_did_key],
            verification_methods,
            also_known_as: vec![format!("at://{handle}")],
            services,
            prev: None,
            sig: String::new(),
        };

        operation.sig = sign_plc_operation(&operation, &rotation_keypair.secret_key)
            .context("signing PLC operation")?;

        let did = match self
            .plc_client
            .create_did(&operation)
            .await
            .context("creating did:plc via PLC directory")
        {
            Ok(did) => did,
            Err(err) => {
                self.link_store
                    .mark_link_failed(nostr_pubkey, None, &err.to_string())
                    .await
                    .context("recording PLC provisioning failure")?;
                return Err(err);
            }
        };

        let record = self
            .create_pds_account_and_mark_ready(nostr_pubkey, &did, handle)
            .await?;

        Ok(ProvisionResult {
            did,
            handle: record.handle,
            signing_key_id: record.signing_key_id,
        })
    }

    async fn create_pds_account_and_mark_ready(
        &self,
        nostr_pubkey: &str,
        did: &str,
        handle: &str,
    ) -> Result<AccountLinkRecord> {
        match self
            .pds_creator
            .create_account(did, handle)
            .await
            .context("creating account on PDS")
        {
            Ok(()) => self
                .link_store
                .mark_link_ready(nostr_pubkey, did)
                .await
                .context("marking account link ready"),
            Err(err) => {
                self.link_store
                    .mark_link_failed(nostr_pubkey, Some(did), &err.to_string())
                    .await
                    .context("recording PDS provisioning failure")?;
                Err(err)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

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

    struct MockKeyStore {
        fail: bool,
        generated: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl KeyStore for MockKeyStore {
        async fn generate_keypair(&self, purpose: &str) -> Result<(String, KeyPair)> {
            if self.fail {
                bail!("key generation failed");
            }

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
        fail: bool,
        calls: Arc<Mutex<Vec<PlcOperation>>>,
    }

    #[async_trait]
    impl PlcClient for MockPlcClient {
        async fn create_did(&self, operation: &PlcOperation) -> Result<String> {
            if self.fail {
                bail!("PLC directory unavailable");
            }

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

    struct MockLinkStore {
        links: SharedLinks,
    }

    #[async_trait]
    impl AccountLinkStore for MockLinkStore {
        async fn get_link_by_pubkey(&self, nostr_pubkey: &str) -> Result<Option<AccountLinkRecord>> {
            Ok(self.links.records.lock().unwrap().get(nostr_pubkey).cloned())
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

    fn make_provisioner(
        links: SharedLinks,
        plc_fail: bool,
        pds_fail: bool,
    ) -> (
        AccountProvisioner<MockKeyStore, MockPlcClient, MockPdsCreator, MockLinkStore>,
        Arc<Mutex<Vec<String>>>,
        Arc<Mutex<Vec<PlcOperation>>>,
        Arc<Mutex<Vec<(String, String)>>>,
    ) {
        let generated = Arc::new(Mutex::new(Vec::new()));
        let plc_calls = Arc::new(Mutex::new(Vec::new()));
        let pds_calls = Arc::new(Mutex::new(Vec::new()));

        let provisioner = AccountProvisioner {
            key_store: MockKeyStore {
                fail: false,
                generated: generated.clone(),
            },
            plc_client: MockPlcClient {
                fail: plc_fail,
                calls: plc_calls.clone(),
            },
            pds_creator: MockPdsCreator {
                fail: pds_fail,
                calls: pds_calls.clone(),
            },
            link_store: MockLinkStore { links },
            pds_endpoint: "https://pds.divine.video".to_string(),
            handle_domain: "divine.video".to_string(),
        };

        (provisioner, generated, plc_calls, pds_calls)
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

    #[tokio::test]
    async fn successful_provisioning_flow_uses_distinct_keys() {
        let links = SharedLinks::default();
        let (provisioner, generated, plc_calls, pds_calls) =
            make_provisioner(links.clone(), false, false);

        let result = provisioner
            .provision_account("npub_abc123", "alice.divine.video")
            .await
            .expect("provisioning should succeed");

        let stored = links.get("npub_abc123");
        assert_eq!(stored.provisioning_state, ProvisioningState::Ready);
        assert_eq!(stored.did.as_deref(), Some("did:plc:testaccount"));
        assert_ne!(stored.signing_key_id, stored.plc_rotation_key_ref);
        assert_eq!(generated.lock().unwrap().len(), 2);
        assert_eq!(plc_calls.lock().unwrap().len(), 1);
        assert_eq!(pds_calls.lock().unwrap().len(), 1);
        assert_eq!(result.signing_key_id, stored.signing_key_id);
    }

    #[tokio::test]
    async fn duplicate_handle_returns_error() {
        let links = SharedLinks::default();
        links.insert(failed_record("npub_taken", "alice.divine.video"));

        let (provisioner, generated, plc_calls, pds_calls) =
            make_provisioner(links, false, false);

        let err = provisioner
            .provision_account("npub_other", "alice.divine.video")
            .await
            .expect_err("duplicate handle should fail");

        assert!(err.to_string().contains("handle already taken"));
        assert!(generated.lock().unwrap().is_empty());
        assert!(plc_calls.lock().unwrap().is_empty());
        assert!(pds_calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn pds_failure_marks_link_failed() {
        let links = SharedLinks::default();
        let (provisioner, _generated, plc_calls, pds_calls) =
            make_provisioner(links.clone(), false, true);

        let err = provisioner
            .provision_account("npub_fail", "carol.divine.video")
            .await
            .expect_err("PDS failure should bubble up");

        let stored = links.get("npub_fail");
        assert_eq!(stored.provisioning_state, ProvisioningState::Failed);
        assert_eq!(stored.did.as_deref(), Some("did:plc:testaccount"));
        assert!(stored
            .provisioning_error
            .as_deref()
            .unwrap_or_default()
            .contains("PDS"));
        assert_eq!(plc_calls.lock().unwrap().len(), 1);
        assert_eq!(pds_calls.lock().unwrap().len(), 1);
        assert!(err.to_string().contains("creating account on PDS"));
    }

    #[tokio::test]
    async fn retry_reuses_existing_failed_link() {
        let links = SharedLinks::default();
        links.insert(failed_record("npub_retry", "dana.divine.video"));

        let (provisioner, generated, plc_calls, pds_calls) =
            make_provisioner(links.clone(), false, false);

        let result = provisioner
            .provision_account("npub_retry", "dana.divine.video")
            .await
            .expect("retry should recover");

        let stored = links.get("npub_retry");
        assert_eq!(stored.provisioning_state, ProvisioningState::Ready);
        assert_eq!(stored.did.as_deref(), Some("did:plc:retryme"));
        assert!(generated.lock().unwrap().is_empty());
        assert!(plc_calls.lock().unwrap().is_empty());
        assert_eq!(pds_calls.lock().unwrap().len(), 1);
        assert_eq!(result.did, "did:plc:retryme");
    }

    #[test]
    fn pubkey_to_did_key_format() {
        let secp = Secp256k1::new();
        let secret = SecretKey::from_slice(&[0x01; 32]).unwrap();
        let public = PublicKey::from_secret_key(&secp, &secret);

        let did_key = pubkey_to_did_key(&public);
        assert!(did_key.starts_with("did:key:z"));
        assert_eq!(did_key, pubkey_to_did_key(&public));
    }
}
