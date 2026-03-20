use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use diesel::Connection;
use diesel::PgConnection;
use divine_bridge_db::{
    get_account_link_lifecycle, get_account_link_lifecycle_by_handle, mark_account_link_failed,
    mark_account_link_ready, upsert_pending_account_link,
};
use secp256k1::rand::rngs::OsRng;
use secp256k1::Secp256k1;

use crate::provisioner::{
    AccountLinkRecord, AccountLinkStore, KeyPair, KeyStore, PendingAccountLink, ProvisioningState,
};

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
