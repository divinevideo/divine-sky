use anyhow::{Context, Result};
use diesel::PgConnection;
use divine_bridge_db::{
    build_pool, disable_account_link, enable_account_link, get_account_link_lifecycle,
    list_publish_status_for_events, mark_account_link_failed, mark_account_link_ready,
    upsert_pending_account_link, DbPool,
};

use crate::AccountLinkRecord;

#[derive(Clone)]
pub struct DbStore {
    pool: DbPool,
}

impl DbStore {
    pub fn connect(database_url: &str) -> Result<Self> {
        Ok(Self {
            pool: build_pool(database_url)?,
        })
    }

    pub async fn upsert_pending_opt_in(
        &self,
        nostr_pubkey: &str,
        handle: &str,
        crosspost_enabled: bool,
    ) -> Result<AccountLinkRecord> {
        let nostr_pubkey = nostr_pubkey.to_string();
        let handle = handle.to_string();
        let signing_key_id = format!("pending-signing:{nostr_pubkey}");
        let plc_rotation_key_ref = format!("pending-rotation:{nostr_pubkey}");
        let row = self
            .with_connection(move |connection| {
                upsert_pending_account_link(
                    connection,
                    &nostr_pubkey,
                    &handle,
                    &signing_key_id,
                    &plc_rotation_key_ref,
                    crosspost_enabled,
                )
            })
            .await?;
        Ok(AccountLinkRecord::from(row))
    }

    pub async fn mark_ready(&self, nostr_pubkey: &str, did: &str) -> Result<AccountLinkRecord> {
        let nostr_pubkey = nostr_pubkey.to_string();
        let did = did.to_string();
        let row = self
            .with_connection(move |connection| {
                mark_account_link_ready(connection, &nostr_pubkey, &did)
            })
            .await?;
        Ok(AccountLinkRecord::from(row))
    }

    pub async fn mark_failed(
        &self,
        nostr_pubkey: &str,
        did: Option<&str>,
        error: &str,
    ) -> Result<AccountLinkRecord> {
        let nostr_pubkey = nostr_pubkey.to_string();
        let did = did.map(str::to_string);
        let error = error.to_string();
        let row = self
            .with_connection(move |connection| {
                mark_account_link_failed(connection, &nostr_pubkey, did.as_deref(), &error)
            })
            .await?;
        Ok(AccountLinkRecord::from(row))
    }

    pub async fn get_by_pubkey(&self, nostr_pubkey: &str) -> Result<Option<AccountLinkRecord>> {
        let nostr_pubkey = nostr_pubkey.to_string();
        let row = self
            .with_connection(move |connection| {
                get_account_link_lifecycle(connection, &nostr_pubkey)
            })
            .await?;
        Ok(row.map(AccountLinkRecord::from))
    }

    pub async fn disable(&self, nostr_pubkey: &str) -> Result<Option<AccountLinkRecord>> {
        if self.get_by_pubkey(nostr_pubkey).await?.is_none() {
            return Ok(None);
        }
        let nostr_pubkey = nostr_pubkey.to_string();
        let row = self
            .with_connection(move |connection| disable_account_link(connection, &nostr_pubkey))
            .await?;
        Ok(Some(AccountLinkRecord::from(row)))
    }

    pub async fn enable(&self, nostr_pubkey: &str) -> Result<Option<AccountLinkRecord>> {
        if self.get_by_pubkey(nostr_pubkey).await?.is_none() {
            return Ok(None);
        }
        let nostr_pubkey = nostr_pubkey.to_string();
        let row = self
            .with_connection(move |connection| enable_account_link(connection, &nostr_pubkey))
            .await?;
        Ok(Some(AccountLinkRecord::from(row)))
    }

    pub async fn list_crosspost_status(
        &self,
        nostr_pubkey: &str,
        event_ids: &[String],
    ) -> Result<Vec<divine_bridge_db::models::PublishStatusRow>> {
        let nostr_pubkey = nostr_pubkey.to_string();
        let event_ids = event_ids.to_vec();
        self.with_connection(move |connection| {
            list_publish_status_for_events(connection, &nostr_pubkey, &event_ids)
        })
        .await
    }

    async fn with_connection<T>(
        &self,
        f: impl FnOnce(&mut PgConnection) -> Result<T> + Send + 'static,
    ) -> Result<T>
    where
        T: Send + 'static,
    {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let mut connection = pool
                .get()
                .context("failed to check out PostgreSQL connection")?;
            f(&mut connection)
        })
        .await
        .context("database task panicked")?
    }
}
