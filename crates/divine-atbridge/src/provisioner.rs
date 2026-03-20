//! Account Provisioner — creates ATProto accounts for Nostr users.
//!
//! Provisions a new ATProto identity by:
//! 1. Generating a secp256k1 signing keypair
//! 2. Creating a did:plc via the PLC directory
//! 3. Creating an account on the rsky-pds
//! 4. Storing the account link in the database
//!
//! All external dependencies are behind traits for testability.

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use secp256k1::{PublicKey, Secp256k1, SecretKey};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Result of successfully provisioning a new ATProto account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionResult {
    pub did: String,
    pub handle: String,
    pub signing_key_id: String,
}

/// A generated keypair for ATProto signing.
#[derive(Debug, Clone)]
pub struct KeyPair {
    pub secret_key: SecretKey,
    pub public_key: PublicKey,
}

/// PLC operation sent to the PLC directory.
///
/// Field order matters for DAG-CBOR canonical encoding (sorted keys).
/// The `sig` field is excluded when computing the hash for signing and DID derivation.
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

/// Generates and stores signing keys.
#[async_trait]
pub trait KeyStore: Send + Sync {
    /// Generate a new secp256k1 keypair and store it.
    /// Returns a key identifier for later retrieval.
    async fn generate_keypair(&self) -> Result<(String, KeyPair)>;
}

/// Interacts with the PLC directory to create DIDs.
#[async_trait]
pub trait PlcClient: Send + Sync {
    /// Create a new did:plc by posting a signed genesis operation.
    /// Returns the newly created DID.
    async fn create_did(&self, operation: &PlcOperation) -> Result<String>;
}

/// Creates accounts on the PDS.
#[async_trait]
pub trait PdsAccountCreator: Send + Sync {
    /// Create an account on the PDS for the given DID and handle.
    async fn create_account(&self, did: &str, handle: &str) -> Result<()>;
}

/// Stores the link between a Nostr pubkey and an ATProto DID.
#[async_trait]
pub trait AccountLinkStore: Send + Sync {
    /// Check whether a handle is already taken.
    async fn handle_exists(&self, handle: &str) -> Result<bool>;

    /// Store the link between a Nostr pubkey and an ATProto account.
    async fn store_link(
        &self,
        nostr_pubkey: &str,
        did: &str,
        handle: &str,
        signing_key_id: &str,
    ) -> Result<()>;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Encode a secp256k1 compressed public key as a `did:key:z...` string.
///
/// Uses the multicodec prefix 0xe7 for secp256k1-pub and base58btc (multibase 'z').
pub fn pubkey_to_did_key(pubkey: &PublicKey) -> String {
    let compressed = pubkey.serialize(); // 33 bytes
    // multicodec varint for secp256k1-pub: 0xe7 0x01
    let mut buf = vec![0xe7u8, 0x01];
    buf.extend_from_slice(&compressed);
    let encoded = bs58::encode(&buf).into_string();
    format!("did:key:z{encoded}")
}

/// Serialize a PLC operation to DAG-CBOR with the `sig` field excluded.
///
/// This is the canonical unsigned representation used for both signing and DID derivation.
fn unsigned_operation_bytes(operation: &PlcOperation) -> Vec<u8> {
    // Build a value without the sig field
    let mut val = serde_json::to_value(operation).expect("PlcOperation serialises to JSON");
    if let Some(obj) = val.as_object_mut() {
        obj.remove("sig");
    }
    serde_ipld_dagcbor::to_vec(&val).expect("DAG-CBOR encoding cannot fail for valid JSON value")
}

/// Derive the did:plc from a genesis operation.
///
/// did:plc is SHA-256 of the DAG-CBOR-encoded unsigned operation, truncated
/// to 15 bytes, then base32-lower-no-pad encoded.
pub fn derive_did_plc(operation: &PlcOperation) -> String {
    use sha2::{Digest, Sha256};
    let cbor_bytes = unsigned_operation_bytes(operation);
    let hash = Sha256::digest(&cbor_bytes);
    let truncated = &hash[..15];
    let encoded = data_encoding::BASE32_NOPAD
        .encode(truncated)
        .to_ascii_lowercase();
    format!("did:plc:{encoded}")
}

/// Sign a PLC operation with the given secret key.
///
/// Computes SHA-256 of the DAG-CBOR-encoded unsigned operation, then signs
/// with ECDSA (low-S normalized). Returns base64url-no-pad encoded signature.
pub fn sign_plc_operation(
    operation: &PlcOperation,
    secret_key: &SecretKey,
) -> Result<String> {
    use sha2::{Digest, Sha256};

    let cbor_bytes = unsigned_operation_bytes(operation);
    let hash: [u8; 32] = Sha256::digest(&cbor_bytes).into();
    let msg = secp256k1::Message::from_digest(hash);
    let secp = Secp256k1::new();
    let sig = secp.sign_ecdsa(&msg, secret_key);

    // Encode as base64url-no-pad (ATProto convention)
    let sig_bytes = sig.serialize_compact(); // 64 bytes (r || s)
    Ok(data_encoding::BASE64URL_NOPAD.encode(&sig_bytes))
}

// ---------------------------------------------------------------------------
// AccountProvisioner
// ---------------------------------------------------------------------------

/// Orchestrates the full account provisioning flow.
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
    /// Provision a new ATProto account linked to a Nostr pubkey.
    ///
    /// `handle` should be the bare handle (e.g. `alice.divine.video`).
    pub async fn provision_account(
        &self,
        nostr_pubkey: &str,
        handle: &str,
    ) -> Result<ProvisionResult> {
        // 1. Check for duplicate handle
        if self
            .link_store
            .handle_exists(handle)
            .await
            .context("checking handle uniqueness")?
        {
            bail!("handle already taken: {handle}");
        }

        // 2. Generate signing keypair
        let (key_id, keypair) = self
            .key_store
            .generate_keypair()
            .await
            .context("generating signing keypair")?;

        let did_key = pubkey_to_did_key(&keypair.public_key);

        // 3. Build PLC genesis operation
        let mut verification_methods = std::collections::BTreeMap::new();
        verification_methods.insert("atproto".to_string(), did_key.clone());

        let mut services = std::collections::BTreeMap::new();
        services.insert(
            "atproto_pds".to_string(),
            PlcService {
                service_type: "AtprotoPersonalDataServer".to_string(),
                endpoint: self.pds_endpoint.clone(),
            },
        );

        // Build unsigned operation, then sign it
        let mut operation = PlcOperation {
            op_type: "plc_operation".to_string(),
            rotation_keys: vec![did_key.clone()],
            verification_methods,
            also_known_as: vec![format!("at://{handle}")],
            services,
            prev: None,
            sig: String::new(),
        };

        // Sign with the rotation key
        operation.sig = sign_plc_operation(&operation, &keypair.secret_key)
            .context("failed to sign PLC operation")?;

        // 4. Create DID via PLC directory
        let did = self
            .plc_client
            .create_did(&operation)
            .await
            .context("creating did:plc via PLC directory")?;

        // 5. Create account on PDS
        self.pds_creator
            .create_account(&did, handle)
            .await
            .context("creating account on PDS")?;

        // 6. Store account link
        self.link_store
            .store_link(nostr_pubkey, &did, handle, &key_id)
            .await
            .context("storing account link")?;

        Ok(ProvisionResult {
            did,
            handle: handle.to_string(),
            signing_key_id: key_id,
        })
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio;

    // -- In-memory mock implementations -------------------------------------

    /// In-memory key store that returns a deterministic keypair.
    struct MockKeyStore {
        fail: bool,
    }

    #[async_trait]
    impl KeyStore for MockKeyStore {
        async fn generate_keypair(&self) -> Result<(String, KeyPair)> {
            if self.fail {
                bail!("key generation failed");
            }
            let secp = Secp256k1::new();
            // deterministic key for testing (32 bytes, all 0x01)
            let secret = SecretKey::from_slice(&[0x01; 32]).unwrap();
            let public = PublicKey::from_secret_key(&secp, &secret);
            Ok((
                "key-001".to_string(),
                KeyPair {
                    secret_key: secret,
                    public_key: public,
                },
            ))
        }
    }

    /// Mock PLC client that records calls and returns a deterministic DID.
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
            // Return a derived DID for determinism
            Ok(derive_did_plc(operation))
        }
    }

    /// Mock PDS account creator.
    struct MockPdsCreator {
        fail: bool,
        calls: Arc<Mutex<Vec<(String, String)>>>,
    }

    #[async_trait]
    impl PdsAccountCreator for MockPdsCreator {
        async fn create_account(&self, did: &str, handle: &str) -> Result<()> {
            if self.fail {
                bail!("PDS account creation failed");
            }
            self.calls
                .lock()
                .unwrap()
                .push((did.to_string(), handle.to_string()));
            Ok(())
        }
    }

    /// Mock account link store with in-memory state.
    struct MockLinkStore {
        handles: Arc<Mutex<Vec<String>>>,
        links: Arc<Mutex<Vec<(String, String, String, String)>>>,
    }

    impl MockLinkStore {
        fn new() -> Self {
            Self {
                handles: Arc::new(Mutex::new(Vec::new())),
                links: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn with_existing_handle(handle: &str) -> Self {
            Self {
                handles: Arc::new(Mutex::new(vec![handle.to_string()])),
                links: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl AccountLinkStore for MockLinkStore {
        async fn handle_exists(&self, handle: &str) -> Result<bool> {
            Ok(self.handles.lock().unwrap().contains(&handle.to_string()))
        }

        async fn store_link(
            &self,
            nostr_pubkey: &str,
            did: &str,
            handle: &str,
            signing_key_id: &str,
        ) -> Result<()> {
            self.links.lock().unwrap().push((
                nostr_pubkey.to_string(),
                did.to_string(),
                handle.to_string(),
                signing_key_id.to_string(),
            ));
            Ok(())
        }
    }

    // -- Helper to build a provisioner with configurable mocks ---------------

    fn make_provisioner(
        key_fail: bool,
        plc_fail: bool,
        pds_fail: bool,
        link_store: MockLinkStore,
    ) -> (
        AccountProvisioner<MockKeyStore, MockPlcClient, MockPdsCreator, MockLinkStore>,
        Arc<Mutex<Vec<PlcOperation>>>,
        Arc<Mutex<Vec<(String, String)>>>,
    ) {
        let plc_calls = Arc::new(Mutex::new(Vec::new()));
        let pds_calls = Arc::new(Mutex::new(Vec::new()));

        let provisioner = AccountProvisioner {
            key_store: MockKeyStore { fail: key_fail },
            plc_client: MockPlcClient {
                fail: plc_fail,
                calls: plc_calls.clone(),
            },
            pds_creator: MockPdsCreator {
                fail: pds_fail,
                calls: pds_calls.clone(),
            },
            link_store,
            pds_endpoint: "https://pds.divine.video".to_string(),
            handle_domain: ".divine.video".to_string(),
        };

        (provisioner, plc_calls, pds_calls)
    }

    // -- Tests ---------------------------------------------------------------

    #[tokio::test]
    async fn successful_provisioning_flow() {
        let link_store = MockLinkStore::new();
        let links = link_store.links.clone();
        let (provisioner, plc_calls, pds_calls) =
            make_provisioner(false, false, false, link_store);

        let result = provisioner
            .provision_account("npub_abc123", "alice.divine.video")
            .await
            .expect("provisioning should succeed");

        // Returns valid result
        assert!(result.did.starts_with("did:plc:"));
        assert_eq!(result.handle, "alice.divine.video");
        assert_eq!(result.signing_key_id, "key-001");

        // PLC directory was called with correct operation
        let plc = plc_calls.lock().unwrap();
        assert_eq!(plc.len(), 1);
        assert_eq!(plc[0].op_type, "plc_operation");
        assert_eq!(plc[0].also_known_as, vec!["at://alice.divine.video"]);
        assert!(plc[0].rotation_keys[0].starts_with("did:key:z"));
        assert!(plc[0]
            .verification_methods
            .get("atproto")
            .unwrap()
            .starts_with("did:key:z"));
        assert_eq!(
            plc[0]
                .services
                .get("atproto_pds")
                .unwrap()
                .endpoint,
            "https://pds.divine.video"
        );

        // PDS account was created
        let pds = pds_calls.lock().unwrap();
        assert_eq!(pds.len(), 1);
        assert_eq!(pds[0].0, result.did);
        assert_eq!(pds[0].1, "alice.divine.video");

        // Link was stored
        let stored = links.lock().unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].0, "npub_abc123");
        assert_eq!(stored[0].1, result.did);
        assert_eq!(stored[0].2, "alice.divine.video");
        assert_eq!(stored[0].3, "key-001");
    }

    #[tokio::test]
    async fn duplicate_handle_returns_error() {
        let link_store = MockLinkStore::with_existing_handle("alice.divine.video");
        let (provisioner, plc_calls, pds_calls) =
            make_provisioner(false, false, false, link_store);

        let err = provisioner
            .provision_account("npub_abc123", "alice.divine.video")
            .await
            .expect_err("should fail for duplicate handle");

        assert!(
            err.to_string().contains("handle already taken"),
            "error message was: {err}"
        );

        // Nothing else should have been called
        assert!(plc_calls.lock().unwrap().is_empty());
        assert!(pds_calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn plc_directory_failure_returns_error() {
        let link_store = MockLinkStore::new();
        let (provisioner, _plc_calls, pds_calls) =
            make_provisioner(false, true, false, link_store);

        let err = provisioner
            .provision_account("npub_abc123", "bob.divine.video")
            .await
            .expect_err("should fail when PLC directory is down");

        assert!(
            err.to_string().contains("PLC directory")
                || err.to_string().contains("creating did:plc"),
            "error message was: {err}"
        );

        // PDS should NOT have been called since PLC failed first
        assert!(pds_calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn pds_account_creation_failure_returns_error() {
        let link_store = MockLinkStore::new();
        let links = link_store.links.clone();
        let (provisioner, plc_calls, _pds_calls) =
            make_provisioner(false, false, true, link_store);

        let err = provisioner
            .provision_account("npub_abc123", "carol.divine.video")
            .await
            .expect_err("should fail when PDS account creation fails");

        assert!(
            err.to_string().contains("PDS account creation")
                || err.to_string().contains("creating account on PDS"),
            "error message was: {err}"
        );

        // PLC was called (key + DID created before PDS failure)
        assert_eq!(plc_calls.lock().unwrap().len(), 1);

        // Link should NOT have been stored since PDS failed
        assert!(links.lock().unwrap().is_empty());
    }

    #[test]
    fn pubkey_to_did_key_format() {
        let secp = Secp256k1::new();
        let secret = SecretKey::from_slice(&[0x01; 32]).unwrap();
        let public = PublicKey::from_secret_key(&secp, &secret);

        let did_key = pubkey_to_did_key(&public);
        assert!(did_key.starts_with("did:key:z"));
        // Should be deterministic
        assert_eq!(did_key, pubkey_to_did_key(&public));
    }
}
