use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use diesel::Connection;
use diesel::PgConnection;
use divine_bridge_db::{
    build_pool, disable_account_link, enable_account_link, get_account_link_lifecycle,
    list_publish_status_for_events, mark_account_link_failed, mark_account_link_ready,
    upsert_pending_account_link, DbPool,
};

use crate::AccountLinkRecord;

#[derive(Clone)]
pub struct DbStore {
    connection: ConnectionMode,
}

#[derive(Clone)]
enum ConnectionMode {
    Pool(DbPool),
    Single(Arc<Mutex<PgConnection>>),
}

impl DbStore {
    pub fn connect(database_url: &str) -> Result<Self> {
        Ok(Self {
            connection: ConnectionMode::Pool(build_pool(database_url)?),
        })
    }

    pub fn connect_single(database_url: &str) -> Result<Self> {
        let connection =
            PgConnection::establish(database_url).context("failed to connect to PostgreSQL")?;
        Ok(Self {
            connection: ConnectionMode::Single(Arc::new(Mutex::new(connection))),
        })
    }

    pub fn upsert_pending_opt_in(
        &self,
        nostr_pubkey: &str,
        handle: &str,
        crosspost_enabled: bool,
    ) -> Result<AccountLinkRecord> {
        let signing_key_id = format!("pending-signing:{nostr_pubkey}");
        let plc_rotation_key_ref = format!("pending-rotation:{nostr_pubkey}");
        let row = self.with_connection(|connection| {
            upsert_pending_account_link(
                connection,
                nostr_pubkey,
                handle,
                &signing_key_id,
                &plc_rotation_key_ref,
                crosspost_enabled,
            )
        })?;
        Ok(AccountLinkRecord::from(row))
    }

    pub fn mark_ready(&self, nostr_pubkey: &str, did: &str) -> Result<AccountLinkRecord> {
        let row = self
            .with_connection(|connection| mark_account_link_ready(connection, nostr_pubkey, did))?;
        Ok(AccountLinkRecord::from(row))
    }

    pub fn mark_failed(
        &self,
        nostr_pubkey: &str,
        did: Option<&str>,
        error: &str,
    ) -> Result<AccountLinkRecord> {
        let row = self.with_connection(|connection| {
            mark_account_link_failed(connection, nostr_pubkey, did, error)
        })?;
        Ok(AccountLinkRecord::from(row))
    }

    pub fn get_by_pubkey(&self, nostr_pubkey: &str) -> Result<Option<AccountLinkRecord>> {
        let row = self
            .with_connection(|connection| get_account_link_lifecycle(connection, nostr_pubkey))?;
        Ok(row.map(AccountLinkRecord::from))
    }

    pub fn disable(&self, nostr_pubkey: &str) -> Result<Option<AccountLinkRecord>> {
        if self.get_by_pubkey(nostr_pubkey)?.is_none() {
            return Ok(None);
        }
        let row =
            self.with_connection(|connection| disable_account_link(connection, nostr_pubkey))?;
        Ok(Some(AccountLinkRecord::from(row)))
    }

    pub fn enable(&self, nostr_pubkey: &str) -> Result<Option<AccountLinkRecord>> {
        if self.get_by_pubkey(nostr_pubkey)?.is_none() {
            return Ok(None);
        }
        let row =
            self.with_connection(|connection| enable_account_link(connection, nostr_pubkey))?;
        Ok(Some(AccountLinkRecord::from(row)))
    }

    pub fn list_crosspost_status(
        &self,
        nostr_pubkey: &str,
        event_ids: &[String],
    ) -> Result<Vec<divine_bridge_db::models::PublishStatusRow>> {
        self.with_connection(|connection| {
            list_publish_status_for_events(connection, nostr_pubkey, event_ids)
        })
    }

    fn with_connection<T>(&self, f: impl FnOnce(&mut PgConnection) -> Result<T>) -> Result<T> {
        match &self.connection {
            ConnectionMode::Pool(pool) => {
                let mut connection = pool
                    .get()
                    .context("failed to check out PostgreSQL connection")?;
                f(&mut connection)
            }
            ConnectionMode::Single(connection) => {
                let mut connection = connection.lock().unwrap();
                f(&mut connection)
            }
        }
    }
}
