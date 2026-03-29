use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use diesel::Connection;
use diesel::PgConnection;
use divine_bridge_db::models::{
    AccountLinkLifecycleRow, NewAssetManifestEntry, NewRecordMapping, UpsertIngestOffset,
};
use divine_bridge_db::{
    get_account_link_lifecycle, get_ingest_offset, get_publish_job, get_record_mapping,
    insert_asset, insert_record_mapping,
    update_record_mapping_status as update_record_mapping_status_query, upsert_ingest_offset,
};
use divine_bridge_types::RecordStatus;

use crate::backfill_planner::{BackfillPlanner, BackfillRelayConnector};
use crate::config::BridgeConfig;
use crate::config::{DEFAULT_BACKFILL_BATCH_SIZE, DEFAULT_BACKFILL_PLANNER_INTERVAL_SECS};
use crate::health::RuntimeHealthState;
use crate::nostr_consumer::{
    parse_relay_message, NostrConsumer, RelayConnection, RelayMessage, WebSocketRelayConnection,
};
use crate::pipeline::{
    AccountLink, AccountStore, AssetManifestRecord, BridgePipeline, HttpBlobFetcher, RecordMapping,
    RecordStore,
};
use crate::publisher::PdsClient;
use crate::video_service::VideoServiceUploader;
use crate::runtime_filter;

pub type SharedConnection = Arc<Mutex<PgConnection>>;

#[derive(Clone)]
pub struct DbAccountStore {
    connection: SharedConnection,
}

impl DbAccountStore {
    pub fn new(connection: SharedConnection) -> Self {
        Self { connection }
    }
}

pub fn account_link_from_lifecycle_row(row: &AccountLinkLifecycleRow) -> Option<AccountLink> {
    let did = row.did.clone()?;
    let is_ready = row.provisioning_state == "ready";
    if !is_ready || row.disabled_at.is_some() || !row.crosspost_enabled {
        return None;
    }

    Some(AccountLink {
        nostr_pubkey: row.nostr_pubkey.clone(),
        did,
        opted_in: row.crosspost_enabled && is_ready,
    })
}

#[async_trait::async_trait]
impl AccountStore for DbAccountStore {
    async fn get_account_link(&self, nostr_pubkey: &str) -> Result<Option<AccountLink>> {
        let mut connection = self.connection.lock().unwrap();
        let row = get_account_link_lifecycle(&mut connection, nostr_pubkey)?;
        Ok(row.and_then(|row| account_link_from_lifecycle_row(&row)))
    }
}

#[derive(Clone)]
pub struct DbRecordStore {
    connection: SharedConnection,
}

impl DbRecordStore {
    pub fn new(connection: SharedConnection) -> Self {
        Self { connection }
    }
}

#[async_trait::async_trait]
impl RecordStore for DbRecordStore {
    async fn is_event_processed(&self, event_id: &str) -> Result<bool> {
        let mut connection = self.connection.lock().unwrap();
        Ok(get_record_mapping(&mut connection, event_id)?.is_some()
            || get_publish_job(&mut connection, event_id)?.is_some())
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelayReadOutcome {
    Message,
    Closed,
    Reconnect,
}

#[derive(Clone, Copy, Debug, Default)]
struct RuntimeBackfillRelayConnector;

#[async_trait::async_trait]
impl BackfillRelayConnector for RuntimeBackfillRelayConnector {
    type Connection = WebSocketRelayConnection;

    async fn connect(&self, relay_url: &str) -> Result<Self::Connection> {
        WebSocketRelayConnection::connect(relay_url).await
    }
}

async fn establish_relay_session<C>(
    relay_result: Result<C>,
    req: String,
    relay_url: &str,
    health: &RuntimeHealthState,
) -> Option<C>
where
    C: RelayConnection,
{
    let mut relay = match relay_result {
        Ok(relay) => relay,
        Err(error) => {
            tracing::warn!(
                error = %error,
                relay_url = %relay_url,
                "failed to connect to relay; retrying"
            );
            health.record_relay_failure(error.to_string());
            return None;
        }
    };

    if let Err(error) = relay.send(req).await {
        tracing::warn!(
            error = %error,
            relay_url = %relay_url,
            "failed to send relay subscription; reconnecting"
        );
        health.record_relay_failure(error.to_string());
        return None;
    }

    health.record_success();
    Some(relay)
}

async fn read_relay_frame<C>(
    relay: &mut C,
    relay_url: &str,
    health: &RuntimeHealthState,
) -> (RelayReadOutcome, Option<String>)
where
    C: RelayConnection,
{
    match relay.recv().await {
        Ok(Some(raw)) => (RelayReadOutcome::Message, Some(raw)),
        Ok(None) => (RelayReadOutcome::Closed, None),
        Err(error) => {
            tracing::warn!(
                error = %error,
                relay_url = %relay_url,
                "failed to read relay frame; reconnecting"
            );
            health.record_relay_failure(error.to_string());
            (RelayReadOutcome::Reconnect, None)
        }
    }
}

pub async fn run_service(config: &BridgeConfig) -> Result<()> {
    run_service_with_state(config, RuntimeHealthState::new()).await
}

pub async fn run_service_with_state(
    config: &BridgeConfig,
    health: RuntimeHealthState,
) -> Result<()> {
    let connection = establish_connection(&config.database_url)?;
    let account_store = DbAccountStore::new(connection.clone());
    let record_store = DbRecordStore::new(connection.clone());
    let blob_fetcher = HttpBlobFetcher::new(Duration::from_secs(60))?;
    let pds_client_for_blobs =
        PdsClient::new(config.pds_url.clone(), config.pds_auth_token.clone());
    let blob_uploader: Box<dyn crate::pipeline::BlobUploader> = if config.video_service_enabled {
        tracing::info!(
            video_service_url = %config.video_service_url,
            "video uploads will be routed through video transcoding service"
        );
        Box::new(VideoServiceUploader::new(
            pds_client_for_blobs,
            config.pds_url.clone(),
            config.pds_auth_token.clone(),
            config.video_service_url.clone(),
            Duration::from_secs(config.video_service_poll_timeout_secs),
            Duration::from_millis(config.video_service_poll_interval_ms),
        ))
    } else {
        Box::new(pds_client_for_blobs)
    };
    let pds_publisher = PdsClient::new(config.pds_url.clone(), config.pds_auth_token.clone());
    let pipeline = Arc::new(BridgePipeline::new(
        account_store,
        record_store,
        blob_fetcher,
        blob_uploader,
        pds_publisher,
    ));
    let backfill_planner = BackfillPlanner::new(
        config.relay_url.clone(),
        connection.clone(),
        pipeline.clone(),
        RuntimeBackfillRelayConnector,
        DEFAULT_BACKFILL_BATCH_SIZE,
    );

    tokio::spawn(async move {
        let mut ticker =
            tokio::time::interval(Duration::from_secs(DEFAULT_BACKFILL_PLANNER_INTERVAL_SECS));
        loop {
            ticker.tick().await;
            if let Err(error) = backfill_planner.run_once().await {
                tracing::warn!(error = %error, "backfill planner run failed");
            }
        }
    });

    loop {
        let mut consumer = NostrConsumer::new(config.relay_url.clone());
        consumer.last_seen_timestamp =
            match load_relay_cursor(&connection, &config.relay_source_name) {
                Ok(cursor) => cursor,
                Err(error) => {
                    tracing::error!(
                        error = %error,
                        source = %config.relay_source_name,
                        "failed to load relay replay cursor"
                    );
                    health.record_runtime_failure(error.to_string());
                    tokio::time::sleep(health.next_retry_delay()).await;
                    continue;
                }
            };

        tracing::info!(
            relay_url = %config.relay_url,
            source = %config.relay_source_name,
            "connecting bridge runtime"
        );
        let req = consumer.build_req(&runtime_filter());
        let mut relay = match establish_relay_session(
            crate::nostr_consumer::WebSocketRelayConnection::connect(&config.relay_url).await,
            req,
            &config.relay_url,
            &health,
        )
        .await
        {
            Some(relay) => relay,
            None => {
                tokio::time::sleep(health.next_retry_delay()).await;
                continue;
            }
        };

        let mut reconnect = false;
        loop {
            let (outcome, raw) = read_relay_frame(&mut relay, &config.relay_url, &health).await;
            match outcome {
                RelayReadOutcome::Message => {}
                RelayReadOutcome::Closed => break,
                RelayReadOutcome::Reconnect => {
                    reconnect = true;
                    break;
                }
            }
            let raw = match raw {
                Some(raw) => raw,
                None => continue,
            };

            match parse_relay_message(&raw) {
                Ok(RelayMessage::Event { event, .. }) => {
                    let event_id = event.id.clone();
                    let created_at = event.created_at;
                    let result = pipeline.process_event(&event).await;
                    match result {
                        crate::pipeline::ProcessResult::Error { message } => {
                            tracing::error!(
                                error = %message,
                                event_id = %event_id,
                                "bridge pipeline rejected relay event"
                            );
                            health.record_processing_failure(message);
                        }
                        _ => {
                            if let Err(error) = persist_relay_cursor(
                                &connection,
                                &config.relay_source_name,
                                &event_id,
                                created_at,
                            ) {
                                tracing::error!(
                                    error = %error,
                                    event_id = %event_id,
                                    "failed to persist relay cursor"
                                );
                                health.record_runtime_failure(error.to_string());
                                continue;
                            }
                            consumer.last_seen_timestamp = Some(created_at);
                            health.record_success();
                        }
                    }
                }
                Ok(RelayMessage::Eose { .. }) => {}
                Ok(RelayMessage::Notice(message)) => {
                    tracing::warn!("relay NOTICE: {message}");
                }
                Ok(RelayMessage::Unknown(_)) => {}
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        relay_url = %config.relay_url,
                        "failed to parse relay frame; continuing"
                    );
                    health.record_processing_failure(error.to_string());
                    continue;
                }
            }
        }

        relay.close().await.ok();
        if !reconnect {
            tracing::warn!("relay connection closed; reconnecting");
            health.record_relay_failure("relay connection closed");
        }

        let delay = health.next_retry_delay();
        tracing::warn!(
            delay_secs = delay.as_secs(),
            "sleeping before relay reconnect"
        );
        tokio::time::sleep(delay).await;
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{anyhow, Result};
    use async_trait::async_trait;

    use super::*;

    struct MockRelayConnection {
        send_error: Option<anyhow::Error>,
        recv_results: Vec<std::result::Result<Option<String>, anyhow::Error>>,
    }

    #[async_trait]
    impl RelayConnection for MockRelayConnection {
        async fn send(&mut self, _msg: String) -> Result<()> {
            match self.send_error.take() {
                Some(error) => Err(error),
                None => Ok(()),
            }
        }

        async fn recv(&mut self) -> Result<Option<String>> {
            if self.recv_results.is_empty() {
                Ok(None)
            } else {
                self.recv_results.remove(0)
            }
        }

        async fn close(&mut self) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn establish_relay_session_retries_connect_failures_without_crashing() {
        let health = RuntimeHealthState::new();

        for _ in 0..2 {
            let relay = establish_relay_session::<MockRelayConnection>(
                Err(anyhow!("connect failed")),
                "REQ".to_string(),
                "wss://relay.example",
                &health,
            )
            .await;

            assert!(relay.is_none());
            assert!(health.is_ready());
        }

        let relay = establish_relay_session::<MockRelayConnection>(
            Err(anyhow!("connect failed")),
            "REQ".to_string(),
            "wss://relay.example",
            &health,
        )
        .await;

        assert!(relay.is_none());
        assert!(!health.is_ready());
    }

    #[tokio::test]
    async fn read_relay_frame_requests_reconnect_on_read_error() {
        let health = RuntimeHealthState::new();
        let mut relay = MockRelayConnection {
            send_error: None,
            recv_results: vec![Err(anyhow!("socket dropped"))],
        };

        let (outcome, raw) = read_relay_frame(&mut relay, "wss://relay.example", &health).await;

        assert_eq!(outcome, RelayReadOutcome::Reconnect);
        assert!(raw.is_none());
        assert!(health.is_ready(), "single read failure should stay ready");
    }
}
