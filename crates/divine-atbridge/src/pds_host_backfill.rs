//! Backfill existing PLC documents to the production PDS host.

use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::plc_directory::PlcDirectoryClient;
use crate::provisioner::{AccountLinkRecord, PlcOperation, ProvisioningState};

pub const PRODUCTION_PDS_ORIGIN: &str = "https://pds.divine.video";
pub const LEGACY_STAGING_PDS_ORIGIN: &str = "https://pds.staging.dvines.org";

#[async_trait]
pub trait ReadyStateSync: Send + Sync {
    async fn sync_ready_state(&self, nostr_pubkey: &str, handle: &str, did: &str) -> Result<()>;
}

#[async_trait]
pub trait PlcMigrationSigner: Send + Sync {
    async fn sign_pds_migration(
        &self,
        account: &AccountLinkRecord,
        current_operation: &PlcOperation,
        target_pds_origin: &str,
    ) -> Result<PlcOperation>;
}

#[derive(Clone)]
pub struct PdsHostBackfill<S, G> {
    plc_client: PlcDirectoryClient,
    ready_state_sync: S,
    signer: G,
}

impl<S, G> PdsHostBackfill<S, G> {
    pub fn new(plc_client: PlcDirectoryClient, ready_state_sync: S, signer: G) -> Self {
        Self {
            plc_client,
            ready_state_sync,
            signer,
        }
    }
}

impl<S, G> PdsHostBackfill<S, G>
where
    S: ReadyStateSync,
    G: PlcMigrationSigner,
{
    pub async fn backfill_ready_account(
        &self,
        account: &AccountLinkRecord,
        current_operation: PlcOperation,
    ) -> Result<()> {
        anyhow::ensure!(
            matches!(account.provisioning_state, ProvisioningState::Ready),
            "backfill only applies to ready accounts"
        );

        let did = account
            .did
            .as_deref()
            .context("ready account is missing a DID")?;

        let current_endpoint = current_pds_endpoint(&current_operation)?;
        if current_endpoint == PRODUCTION_PDS_ORIGIN {
            self.ready_state_sync
                .sync_ready_state(&account.nostr_pubkey, &account.handle, did)
                .await
                .context("refreshing ready state after PLC update")?;
            return Ok(());
        }
        anyhow::ensure!(
            current_endpoint == LEGACY_STAGING_PDS_ORIGIN,
            "backfill only supports the legacy staging PDS host"
        );

        let signed_operation = self
            .signer
            .sign_pds_migration(account, &current_operation, PRODUCTION_PDS_ORIGIN)
            .await
            .context("signing PLC successor operation for PDS migration")?;

        anyhow::ensure!(
            current_pds_endpoint(&signed_operation)? == PRODUCTION_PDS_ORIGIN,
            "signed PLC successor must target the production PDS host"
        );

        self.plc_client
            .update_did(did, &signed_operation)
            .await
            .context("updating PLC service endpoint")?;

        self.ready_state_sync
            .sync_ready_state(&account.nostr_pubkey, &account.handle, did)
            .await
            .context("refreshing ready state after PLC update")?;

        Ok(())
    }
}

fn current_pds_endpoint(operation: &PlcOperation) -> Result<&str> {
    let service = operation
        .services
        .get("atproto_pds")
        .context("PLC operation is missing the atproto_pds service")?;
    anyhow::ensure!(
        service.service_type == "AtprotoPersonalDataServer",
        "atproto_pds service must be an AtprotoPersonalDataServer"
    );
    Ok(service.endpoint.as_str())
}
