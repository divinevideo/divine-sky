use anyhow::{bail, Context, Result};
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use async_trait::async_trait;
use diesel::Connection;
use diesel::PgConnection;
use divine_bridge_db::{
    get_account_link_lifecycle, get_account_link_lifecycle_by_handle, mark_account_link_failed,
    mark_account_link_ready, upsert_pending_account_link, get_provisioning_key,
    insert_provisioning_key,
};
use divine_bridge_db::models::NewProvisioningKey;
use secp256k1::rand::RngCore;
use secp256k1::rand::rngs::OsRng;
use secp256k1::{PublicKey, Secp256k1, SecretKey};

use crate::provisioner::{
    AccountLinkRecord, AccountLinkStore, KeyPair, KeyStore, PendingAccountLink, ProvisioningState,
};

const PROVISIONING_KEY_ENVELOPE_VERSION: u8 = 1;
const AES_GCM_NONCE_LEN: usize = 12;

#[derive(Clone)]
pub struct DbAccountLinkStore {
    database_url: String,
}

impl DbAccountLinkStore {
    pub fn new(database_url: String) -> Self {
        Self { database_url }
    }

    fn connect(&self) -> Result<PgConnection> {
        PgConnection::establish(&self.database_url).context("failed to connect to PostgreSQL")
    }
}

pub struct GeneratedKeyStore;

#[derive(Clone)]
pub struct DbProvisioningKeyStore {
    database_url: String,
    encryption_key: [u8; 32],
}

impl DbProvisioningKeyStore {
    pub fn new(database_url: String, encryption_key: [u8; 32]) -> Self {
        Self {
            database_url,
            encryption_key,
        }
    }

    fn connect(&self) -> Result<PgConnection> {
        PgConnection::establish(&self.database_url).context("failed to connect to PostgreSQL")
    }

    fn cipher(&self) -> Result<Aes256Gcm> {
        Aes256Gcm::new_from_slice(&self.encryption_key)
            .context("failed to initialise provisioning key cipher")
    }

    fn provisioning_aad(key_ref: &str, purpose: &str) -> Vec<u8> {
        format!("{key_ref}:{purpose}").into_bytes()
    }

    fn encrypt_secret(
        &self,
        key_ref: &str,
        purpose: &str,
        secret_key: &SecretKey,
    ) -> Result<Vec<u8>> {
        let cipher = self.cipher()?;
        let mut nonce = [0u8; AES_GCM_NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);

        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                aes_gcm::aead::Payload {
                    msg: &secret_key.secret_bytes(),
                    aad: &Self::provisioning_aad(key_ref, purpose),
                },
            )
            .map_err(|_| anyhow::anyhow!("encrypting provisioning secret"))?;

        let mut envelope = Vec::with_capacity(1 + nonce.len() + ciphertext.len());
        envelope.push(PROVISIONING_KEY_ENVELOPE_VERSION);
        envelope.extend_from_slice(&nonce);
        envelope.extend_from_slice(&ciphertext);
        Ok(envelope)
    }

    fn decrypt_secret(&self, key_ref: &str, purpose: &str, envelope: &[u8]) -> Result<SecretKey> {
        if envelope.len() <= 1 + AES_GCM_NONCE_LEN {
            bail!("encrypted provisioning secret is truncated");
        }
        if envelope[0] != PROVISIONING_KEY_ENVELOPE_VERSION {
            bail!(
                "unsupported provisioning secret envelope version: {}",
                envelope[0]
            );
        }

        let nonce = &envelope[1..1 + AES_GCM_NONCE_LEN];
        let ciphertext = &envelope[1 + AES_GCM_NONCE_LEN..];
        let decrypted = self
            .cipher()?
            .decrypt(
                Nonce::from_slice(nonce),
                aes_gcm::aead::Payload {
                    msg: ciphertext,
                    aad: &Self::provisioning_aad(key_ref, purpose),
                },
            )
            .map_err(|_| anyhow::anyhow!("decrypting provisioning secret"))?;

        SecretKey::from_slice(&decrypted).context("stored provisioning secret is not a valid key")
    }

    fn keypair_from_row(
        &self,
        key_ref: &str,
        purpose: &str,
        public_key_hex: &str,
        encrypted_secret: &[u8],
    ) -> Result<KeyPair> {
        let secret_key = self.decrypt_secret(key_ref, purpose, encrypted_secret)?;
        let secp = Secp256k1::new();
        let public_key = PublicKey::from_secret_key(&secp, &secret_key);
        let derived_hex = hex::encode(public_key.serialize());
        if derived_hex != public_key_hex {
            bail!("stored provisioning key public key does not match decrypted secret");
        }

        Ok(KeyPair {
            secret_key,
            public_key,
        })
    }
}

fn map_state(raw: &str) -> Result<ProvisioningState> {
    match raw {
        "pending" => Ok(ProvisioningState::Pending),
        "ready" => Ok(ProvisioningState::Ready),
        "failed" => Ok(ProvisioningState::Failed),
        "disabled" => Ok(ProvisioningState::Disabled),
        other => bail!("unknown provisioning_state: {other}"),
    }
}

fn map_record(row: divine_bridge_db::models::AccountLinkLifecycleRow) -> Result<AccountLinkRecord> {
    Ok(AccountLinkRecord {
        nostr_pubkey: row.nostr_pubkey,
        did: row.did,
        handle: row.handle,
        crosspost_enabled: row.crosspost_enabled,
        signing_key_id: row
            .signing_key_id
            .context("account link lifecycle row missing signing_key_id")?,
        plc_rotation_key_ref: row
            .plc_rotation_key_ref
            .context("account link lifecycle row missing plc_rotation_key_ref")?,
        provisioning_state: map_state(&row.provisioning_state)?,
        provisioning_error: row.provisioning_error,
        disabled_at: row.disabled_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

#[async_trait]
impl AccountLinkStore for DbAccountLinkStore {
    async fn get_link_by_pubkey(&self, nostr_pubkey: &str) -> Result<Option<AccountLinkRecord>> {
        let mut connection = self.connect()?;
        get_account_link_lifecycle(&mut connection, nostr_pubkey)?
            .map(map_record)
            .transpose()
    }

    async fn get_link_by_handle(&self, handle: &str) -> Result<Option<AccountLinkRecord>> {
        let mut connection = self.connect()?;
        get_account_link_lifecycle_by_handle(&mut connection, handle)?
            .map(map_record)
            .transpose()
    }

    async fn save_pending_link(
        &self,
        pending: PendingAccountLink<'_>,
    ) -> Result<AccountLinkRecord> {
        let mut connection = self.connect()?;
        let row = upsert_pending_account_link(
            &mut connection,
            pending.nostr_pubkey,
            pending.handle,
            pending.signing_key_id,
            pending.plc_rotation_key_ref,
            pending.crosspost_enabled,
        )?;
        map_record(row)
    }

    async fn mark_link_ready(&self, nostr_pubkey: &str, did: &str) -> Result<AccountLinkRecord> {
        let mut connection = self.connect()?;
        let row = mark_account_link_ready(&mut connection, nostr_pubkey, did)?;
        map_record(row)
    }

    async fn mark_link_failed(
        &self,
        nostr_pubkey: &str,
        did: Option<&str>,
        error: &str,
    ) -> Result<AccountLinkRecord> {
        let mut connection = self.connect()?;
        let row = mark_account_link_failed(&mut connection, nostr_pubkey, did, error)?;
        map_record(row)
    }
}

#[async_trait]
impl KeyStore for GeneratedKeyStore {
    async fn generate_keypair(&self, purpose: &str) -> Result<(String, KeyPair)> {
        let secp = Secp256k1::new();
        let mut rng = OsRng;
        let (secret_key, public_key) = secp.generate_keypair(&mut rng);
        let key_id = format!("{purpose}:{}", hex::encode(public_key.serialize()));

        Ok((
            key_id,
            KeyPair {
                secret_key,
                public_key,
            },
        ))
    }
}

#[async_trait]
impl KeyStore for DbProvisioningKeyStore {
    async fn generate_keypair(&self, purpose: &str) -> Result<(String, KeyPair)> {
        let secp = Secp256k1::new();
        let mut rng = OsRng;
        let (secret_key, public_key) = secp.generate_keypair(&mut rng);
        let public_key_hex = hex::encode(public_key.serialize());
        let key_ref = format!("{purpose}:{public_key_hex}");
        let encrypted_secret = self.encrypt_secret(&key_ref, purpose, &secret_key)?;

        let mut connection = self.connect()?;
        insert_provisioning_key(
            &mut connection,
            &NewProvisioningKey {
                key_ref: &key_ref,
                key_purpose: purpose,
                public_key_hex: &public_key_hex,
                encrypted_secret: &encrypted_secret,
            },
        )?;

        Ok((
            key_ref,
            KeyPair {
                secret_key,
                public_key,
            },
        ))
    }

    async fn load_keypair(&self, key_ref: &str) -> Result<Option<KeyPair>> {
        let mut connection = self.connect()?;
        let row = get_provisioning_key(&mut connection, key_ref)?;
        row.map(|row| {
            self.keypair_from_row(
                &row.key_ref,
                &row.key_purpose,
                &row.public_key_hex,
                &row.encrypted_secret,
            )
        })
        .transpose()
    }
}
