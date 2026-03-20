use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use diesel::Connection;
use diesel::PgConnection;
use divine_bridge_db::models::{NewAssetManifestEntry, NewRecordMapping, UpsertIngestOffset};
use divine_bridge_db::{
    get_account_link_lifecycle, get_ingest_offset, get_record_mapping, insert_asset,
    insert_record_mapping, update_record_mapping_status as update_record_mapping_status_query,
    upsert_ingest_offset,
};
use divine_bridge_types::RecordStatus;

use crate::config::BridgeConfig;
use crate::nostr_consumer::{parse_relay_message, NostrConsumer, RelayConnection, RelayMessage};
use crate::pipeline::{
    AccountLink, AccountStore, AssetManifestRecord, BridgePipeline, HttpBlobFetcher, RecordMapping,
    RecordStore,
};
use crate::publisher::PdsClient;
use crate::runtime_filter;

type SharedConnection = Arc<Mutex<PgConnection>>;

#[derive(Clone)]
pub struct DbAccountStore {
    connection: SharedConnection,
}

impl DbAccountStore {
    fn new(connection: SharedConnection) -> Self {
        Self { connection }
    }
}

#[async_trait::async_trait]
impl AccountStore for DbAccountStore {
    async fn get_account_link(&self, nostr_pubkey: &str) -> Result<Option<AccountLink>> {
        let mut connection = self.connection.lock().unwrap();
        let row = get_account_link_lifecycle(&mut connection, nostr_pubkey)?;
        Ok(row.and_then(|row| {
            let did = row.did?;
            if row.provisioning_state != "ready" || row.disabled_at.is_some() {
                return None;
            }

            Some(AccountLink {
                nostr_pubkey: row.nostr_pubkey,
                did,
                opted_in: row.crosspost_enabled || row.provisioning_state == "ready",
            })
        }))
    }
}

#[derive(Clone)]
pub struct DbRecordStore {
    connection: SharedConnection,
}

impl DbRecordStore {
    fn new(connection: SharedConnection) -> Self {
        Self { connection }
    }
}

#[async_trait::async_trait]
impl RecordStore for DbRecordStore {
    async fn is_event_processed(&self, event_id: &str) -> Result<bool> {
        let mut connection = self.connection.lock().unwrap();
        Ok(get_record_mapping(&mut connection, event_id)?.is_some())
    }

    async fn save_record_mapping(&self, mapping: RecordMapping) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        insert_record_mapping(
            &mut connection,
            &NewRecordMapping {
                nostr_event_id: &mapping.nostr_event_id,
                did: &mapping.did,
                collection: &mapping.collection,
                rkey: &mapping.rkey,
                at_uri: &mapping.at_uri,
                cid: None,
                status: RecordStatus::Published.as_str(),
            },
        )?;
        Ok(())
    }

    async fn get_mapping_by_nostr_id(&self, event_id: &str) -> Result<Option<RecordMapping>> {
        let mut connection = self.connection.lock().unwrap();
        Ok(
            get_record_mapping(&mut connection, event_id)?.map(|mapping| RecordMapping {
                nostr_event_id: mapping.nostr_event_id,
                at_uri: mapping.at_uri,
                did: mapping.did,
                collection: mapping.collection,
                rkey: mapping.rkey,
                deleted: mapping.status == RecordStatus::Deleted.as_str(),
            }),
        )
    }

    async fn mark_deleted(&self, event_id: &str) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        update_record_mapping_status_query(
            &mut connection,
            event_id,
            None,
            RecordStatus::Deleted.as_str(),
        )?;
        Ok(())
    }

    async fn save_asset_manifest(&self, entry: AssetManifestRecord) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        insert_asset(
            &mut connection,
            &NewAssetManifestEntry {
                source_sha256: &entry.source_sha256,
                blossom_url: entry.blossom_url.as_deref(),
                at_blob_cid: &entry.at_blob_cid,
                mime: &entry.mime,
                bytes: entry.bytes as i64,
                is_derivative: entry.is_derivative,
            },
        )?;
        Ok(())
    }

    async fn update_record_mapping_status(
        &self,
        event_id: &str,
        cid: Option<&str>,
        status: RecordStatus,
    ) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        update_record_mapping_status_query(&mut connection, event_id, cid, status.as_str())?;
        Ok(())
    }
}

fn establish_connection(database_url: &str) -> Result<SharedConnection> {
    let connection =
        PgConnection::establish(database_url).context("failed to connect to PostgreSQL")?;
    Ok(Arc::new(Mutex::new(connection)))
}

fn load_relay_cursor(connection: &SharedConnection, source_name: &str) -> Result<Option<i64>> {
    let mut connection = connection.lock().unwrap();
    Ok(get_ingest_offset(&mut connection, source_name)?
        .map(|offset| offset.last_created_at.timestamp()))
}

fn persist_relay_cursor(
    connection: &SharedConnection,
    source_name: &str,
    event_id: &str,
    created_at: i64,
) -> Result<()> {
    let timestamp = DateTime::<Utc>::from_timestamp(created_at, 0)
        .context("relay event timestamp is out of range")?;
    let mut connection = connection.lock().unwrap();
    upsert_ingest_offset(
        &mut connection,
        &UpsertIngestOffset {
            source_name,
            last_event_id: event_id,
            last_created_at: timestamp,
        },
    )?;
    Ok(())
}

pub async fn run_service(config: &BridgeConfig) -> Result<()> {
    let connection = establish_connection(&config.database_url)?;
    let account_store = DbAccountStore::new(connection.clone());
    let record_store = DbRecordStore::new(connection.clone());
    let blob_fetcher = HttpBlobFetcher::new(Duration::from_secs(60))?;
    let blob_uploader = PdsClient::new(config.pds_url.clone(), config.pds_auth_token.clone());
    let pds_publisher = PdsClient::new(config.pds_url.clone(), config.pds_auth_token.clone());
    let pipeline = BridgePipeline::new(
        account_store,
        record_store,
        blob_fetcher,
        blob_uploader,
        pds_publisher,
    );

    loop {
        let mut consumer = NostrConsumer::new(config.relay_url.clone());
        consumer.last_seen_timestamp = load_relay_cursor(&connection, &config.relay_source_name)?;

        tracing::info!(
            relay_url = %config.relay_url,
            source = %config.relay_source_name,
            "connecting bridge runtime"
        );
        let mut relay = crate::nostr_consumer::WebSocketRelayConnection::connect(&config.relay_url)
            .await
            .context("failed to connect to relay")?;

        let req = consumer.build_req(&runtime_filter());
        relay
            .send(req)
            .await
            .context("failed to send relay subscription")?;

        while let Some(raw) = relay.recv().await.context("failed to read relay frame")? {
            match parse_relay_message(&raw).context("failed to parse relay frame")? {
                RelayMessage::Event { event, .. } => {
                    let event_id = event.id.clone();
                    let created_at = event.created_at;
                    let result = pipeline.process_event(&event).await;
                    match result {
                        crate::pipeline::ProcessResult::Error { message } => {
                            anyhow::bail!("event processing failed: {message}");
                        }
                        _ => {
                            persist_relay_cursor(
                                &connection,
                                &config.relay_source_name,
                                &event_id,
                                created_at,
                            )?;
                            consumer.last_seen_timestamp = Some(created_at);
                        }
                    }
                }
                RelayMessage::Eose { .. } => {}
                RelayMessage::Notice(message) => {
                    tracing::warn!("relay NOTICE: {message}");
                }
                RelayMessage::Unknown(_) => {}
            }
        }

        relay.close().await.ok();
        tracing::warn!("relay connection closed; reconnecting");
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
