//! Backfill existing PLC documents to the production PDS host.

use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::plc_directory::PlcDirectoryClient;
use crate::provisioner::{AccountLinkRecord, PlcOperation, PlcService, ProvisioningState};

pub const PRODUCTION_PDS_ORIGIN: &str = "https://pds.divine.video";

#[async_trait]
pub trait ReadyStateSync: Send + Sync {
    async fn sync_ready_state(&self, nostr_pubkey: &str, handle: &str, did: &str) -> Result<()>;
}

#[derive(Clone)]
pub struct PdsHostBackfill<S> {
    plc_client: PlcDirectoryClient,
    ready_state_sync: S,
}

impl<S> PdsHostBackfill<S> {
    pub fn new(plc_client: PlcDirectoryClient, ready_state_sync: S) -> Self {
        Self {
            plc_client,
            ready_state_sync,
        }
    }
}

impl<S> PdsHostBackfill<S>
where
    S: ReadyStateSync,
{
    pub async fn backfill_ready_account(
        &self,
        account: &AccountLinkRecord,
        mut operation: PlcOperation,
    ) -> Result<()> {
        anyhow::ensure!(
            matches!(account.provisioning_state, ProvisioningState::Ready),
            "backfill only applies to ready accounts"
        );

        let did = account
            .did
            .as_deref()
            .context("ready account is missing a DID")?;
        rewrite_pds_endpoint(&mut operation);

        self.plc_client
            .update_did(did, &operation)
            .await
            .context("updating PLC service endpoint")?;

        self.ready_state_sync
            .sync_ready_state(&account.nostr_pubkey, &account.handle, did)
            .await
            .context("refreshing ready state after PLC update")?;

        Ok(())
    }
}

fn rewrite_pds_endpoint(operation: &mut PlcOperation) {
    let service = operation
        .services
        .entry("atproto_pds".to_string())
        .or_insert(PlcService {
            service_type: "AtprotoPersonalDataServer".to_string(),
            endpoint: String::new(),
        });
    service.endpoint = PRODUCTION_PDS_ORIGIN.to_string();
}
