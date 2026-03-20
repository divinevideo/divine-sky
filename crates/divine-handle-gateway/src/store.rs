use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use diesel::Connection;
use diesel::PgConnection;
use divine_bridge_db::{
    disable_account_link, get_account_link_lifecycle, get_account_link_lifecycle_by_handle,
    mark_account_link_failed, mark_account_link_ready, upsert_pending_account_link,
};

use crate::AccountLinkRecord;

type SharedConnection = Arc<Mutex<PgConnection>>;

#[derive(Clone)]
pub struct DbStore {
    connection: SharedConnection,
}

impl DbStore {
    pub fn connect(database_url: &str) -> Result<Self> {
        let connection =
            PgConnection::establish(database_url).context("failed to connect to PostgreSQL")?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn upsert_pending_opt_in(
        &self,
        nostr_pubkey: &str,
        handle: &str,
    ) -> Result<AccountLinkRecord> {
        let signing_key_id = format!("pending-signing:{nostr_pubkey}");
        let plc_rotation_key_ref = format!("pending-rotation:{nostr_pubkey}");
        let mut connection = self.connection.lock().unwrap();
        let row = upsert_pending_account_link(
            &mut connection,
            nostr_pubkey,
            handle,
            &signing_key_id,
            &plc_rotation_key_ref,
            true,
        )?;
        Ok(AccountLinkRecord::from(row))
    }

    pub fn mark_ready(&self, nostr_pubkey: &str, did: &str) -> Result<AccountLinkRecord> {
        let mut connection = self.connection.lock().unwrap();
        let row = mark_account_link_ready(&mut connection, nostr_pubkey, did)?;
        Ok(AccountLinkRecord::from(row))
    }

    pub fn mark_failed(
        &self,
        nostr_pubkey: &str,
        did: Option<&str>,
        error: &str,
    ) -> Result<AccountLinkRecord> {
        let mut connection = self.connection.lock().unwrap();
        let row = mark_account_link_failed(&mut connection, nostr_pubkey, did, error)?;
        Ok(AccountLinkRecord::from(row))
    }

    pub fn get_by_pubkey(&self, nostr_pubkey: &str) -> Result<Option<AccountLinkRecord>> {
        let mut connection = self.connection.lock().unwrap();
        let row = get_account_link_lifecycle(&mut connection, nostr_pubkey)?;
        Ok(row.map(AccountLinkRecord::from))
    }

    pub fn get_by_handle(&self, handle: &str) -> Result<Option<AccountLinkRecord>> {
        let mut connection = self.connection.lock().unwrap();
        let row = get_account_link_lifecycle_by_handle(&mut connection, handle)?;
        Ok(row.map(AccountLinkRecord::from))
    }

    pub fn disable(&self, nostr_pubkey: &str) -> Result<Option<AccountLinkRecord>> {
        if self.get_by_pubkey(nostr_pubkey)?.is_none() {
            return Ok(None);
        }
        let mut connection = self.connection.lock().unwrap();
        let row = disable_account_link(&mut connection, nostr_pubkey)?;
        Ok(Some(AccountLinkRecord::from(row)))
    }
}
