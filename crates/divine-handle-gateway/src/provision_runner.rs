use anyhow::{Context, Result};
use diesel::sql_query;
use diesel::sql_types::Text;
use diesel::Connection;
use diesel::PgConnection;
use diesel::QueryableByName;
use diesel::RunQueryDsl;
use divine_bridge_db::list_account_link_lifecycle_for_reconciliation;
use divine_bridge_db::models::AccountLinkLifecycleRow;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::keycast_client::KeycastClient;
use crate::name_server_client::NameServerClient;
use crate::store::DbStore;

#[derive(Clone)]
pub struct ProvisionRunner {
    store: DbStore,
    provisioning_client: ProvisioningClient,
    name_server_client: NameServerClient,
    keycast_client: KeycastClient,
}

#[derive(Clone)]
pub struct ProvisioningClient {
    client: Client,
    provision_url: String,
    bearer_token: Option<String>,
}

#[derive(Debug, Serialize)]
struct ProvisionRequest<'a> {
    nostr_pubkey: &'a str,
    handle: &'a str,
}

#[derive(Debug, Deserialize)]
struct ProvisionResponse {
    did: String,
}

#[derive(Debug, QueryableByName)]
struct PendingProvisionRow {
    #[diesel(sql_type = Text)]
    nostr_pubkey: String,
    #[diesel(sql_type = Text)]
    handle: String,
}

impl ProvisioningClient {
    pub fn new(provision_url: String, bearer_token: Option<String>) -> Self {
        Self {
            client: Client::new(),
            provision_url,
            bearer_token,
        }
    }

    async fn provision_account(
        &self,
        nostr_pubkey: &str,
        handle: &str,
    ) -> Result<ProvisionResponse> {
        let mut request = self
            .client
            .post(&self.provision_url)
            .json(&ProvisionRequest {
                nostr_pubkey,
                handle,
            });
        if let Some(token) = &self.bearer_token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .context("provisioning request failed")?
            .error_for_status()
            .context("provisioning request returned non-success status")?;

        Ok(response
            .json::<ProvisionResponse>()
            .await
            .context("failed to decode provisioning response")?)
    }
}

impl ProvisionRunner {
    pub fn new(
        store: DbStore,
        provisioning_client: ProvisioningClient,
        name_server_client: NameServerClient,
        keycast_client: KeycastClient,
    ) -> Self {
        Self {
            store,
            provisioning_client,
            name_server_client,
            keycast_client,
        }
    }

    pub fn enqueue(&self, nostr_pubkey: String, handle: String) {
        let runner = self.clone();
        tokio::spawn(async move {
            if let Err(error) = runner.run_once(&nostr_pubkey, &handle).await {
                tracing::error!(
                    nostr_pubkey = %nostr_pubkey,
                    handle = %handle,
                    error = %error,
                    "provisioning runner failed",
                );
            }
        });
    }

    pub async fn replay_pending_from_database(&self, database_url: &str) -> Result<usize> {
        let mut connection =
            PgConnection::establish(database_url).context("connecting for provisioning replay")?;
        let pending = sql_query(
            "SELECT nostr_pubkey, handle
             FROM account_links
             WHERE provisioning_state = 'pending'
               AND crosspost_enabled = TRUE
               AND disabled_at IS NULL",
        )
        .load::<PendingProvisionRow>(&mut connection)
        .context("loading pending provisioning rows")?;

        for row in &pending {
            if let Err(error) = self.run_once(&row.nostr_pubkey, &row.handle).await {
                tracing::error!(
                    nostr_pubkey = %row.nostr_pubkey,
                    handle = %row.handle,
                    error = %error,
                    "startup replay failed for pending provisioning row",
                );
            }
        }

        Ok(pending.len())
    }

    pub async fn reconcile_existing_from_database(&self, database_url: &str) -> Result<usize> {
        let mut connection = PgConnection::establish(database_url)
            .context("connecting for startup lifecycle reconciliation")?;
        let rows = list_account_link_lifecycle_for_reconciliation(&mut connection)
            .context("loading lifecycle rows for startup reconciliation")?;

        for row in &rows {
            if let Err(error) = self.reconcile_row(row).await {
                tracing::error!(
                    nostr_pubkey = %row.nostr_pubkey,
                    handle = %row.handle,
                    provisioning_state = %row.provisioning_state,
                    error = %error,
                    "startup reconciliation failed for account link row",
                );
            }
        }

        Ok(rows.len())
    }

    async fn reconcile_row(&self, row: &AccountLinkLifecycleRow) -> Result<()> {
        match row.provisioning_state.as_str() {
            "ready" => {
                let did = row
                    .did
                    .as_deref()
                    .context("ready lifecycle row missing did")?;
                self.sync_ready_state(&row.nostr_pubkey, &row.handle, did)
                    .await
            }
            "failed" => {
                let error = row
                    .provisioning_error
                    .as_deref()
                    .unwrap_or("account provisioning previously failed");
                self.sync_failed_state(&row.nostr_pubkey, &row.handle, error)
                    .await
            }
            "disabled" => {
                self.sync_disabled_state(&row.nostr_pubkey, &row.handle)
                    .await
            }
            state => {
                tracing::warn!(
                    nostr_pubkey = %row.nostr_pubkey,
                    handle = %row.handle,
                    provisioning_state = %state,
                    "skipping unexpected lifecycle state during startup reconciliation",
                );
                Ok(())
            }
        }
    }

    pub async fn sync_ready_state(
        &self,
        nostr_pubkey: &str,
        handle: &str,
        did: &str,
    ) -> Result<()> {
        self.keycast_client
            .sync_ready(nostr_pubkey, did)
            .await
            .context("failed to sync ready state to keycast")?;
        self.name_server_client
            .sync_state_for_handle(handle, Some(did), "ready")
            .await
            .context("failed to sync ready state to name server")?;
        Ok(())
    }

    async fn sync_failed_state(&self, nostr_pubkey: &str, handle: &str, error: &str) -> Result<()> {
        self.keycast_client
            .sync_failed(nostr_pubkey, error)
            .await
            .context("failed to sync failed state to keycast")?;
        self.name_server_client
            .sync_state_for_handle(handle, None, "failed")
            .await
            .context("failed to sync failed state to name server")?;
        Ok(())
    }

    async fn sync_disabled_state(&self, nostr_pubkey: &str, handle: &str) -> Result<()> {
        self.keycast_client
            .sync_disabled(nostr_pubkey)
            .await
            .context("failed to sync disabled state to keycast")?;
        self.name_server_client
            .sync_state_for_handle(handle, None, "disabled")
            .await
            .context("failed to sync disabled state to name server")?;
        Ok(())
    }

    async fn run_once(&self, nostr_pubkey: &str, handle: &str) -> Result<()> {
        match self
            .provisioning_client
            .provision_account(nostr_pubkey, handle)
            .await
        {
            Ok(response) => {
                self.store
                    .mark_ready(nostr_pubkey, &response.did)
                    .context("failed to mark account link ready")?;
                self.sync_ready_state(nostr_pubkey, handle, &response.did)
                    .await?;
            }
            Err(error) => {
                let message = error.to_string();
                self.store
                    .mark_failed(nostr_pubkey, None, &message)
                    .context("failed to mark account link failed")?;
                self.sync_failed_state(nostr_pubkey, handle, &message)
                    .await?;
            }
        }

        Ok(())
    }
}
