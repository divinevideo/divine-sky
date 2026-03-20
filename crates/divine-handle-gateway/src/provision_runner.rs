use anyhow::{Context, Result};
use diesel::sql_query;
use diesel::sql_types::Text;
use diesel::Connection;
use diesel::PgConnection;
use diesel::QueryableByName;
use diesel::RunQueryDsl;
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
                self.keycast_client
                    .sync_ready(nostr_pubkey, &response.did)
                    .await
                    .context("failed to sync ready state to keycast")?;
                self.name_server_client
                    .sync_state_for_handle(handle, Some(&response.did), "ready")
                    .await
                    .context("failed to sync ready state to name server")?;
            }
            Err(error) => {
                let message = error.to_string();
                self.store
                    .mark_failed(nostr_pubkey, None, &message)
                    .context("failed to mark account link failed")?;
                self.keycast_client
                    .sync_failed(nostr_pubkey, &message)
                    .await
                    .context("failed to sync failed state to keycast")?;
                self.name_server_client
                    .sync_state_for_handle(handle, None, "failed")
                    .await
                    .context("failed to sync failed state to name server")?;
            }
        }

        Ok(())
    }
}
