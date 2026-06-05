use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use diesel::Connection;
use diesel::PgConnection;
use divine_bridge_db::models::{
    AccountLinkLifecycleRow, NewAssetManifestEntry, NewPublishJob, NewRecordMapping, PublishJob,
    UpsertIngestOffset,
};
use divine_bridge_db::{
    cancel_publish_job, claim_next_backfill_job, claim_next_live_job, enqueue_publish_job,
    get_account_link_lifecycle, get_account_pds_access_jwt_by_did,
    get_account_pds_refresh_jwt_by_did, get_ingest_offset, get_publish_job, get_record_mapping,
    insert_asset, insert_record_mapping, mark_publish_job_completed, mark_publish_job_failed,
    store_account_pds_session_by_did,
    update_record_mapping_status as update_record_mapping_status_query, upsert_ingest_offset,
};
use divine_bridge_types::{NostrEvent, PublishJobSource, PublishState, RecordStatus};

use crate::backfill_planner::{BackfillPlanner, BackfillRelayConnector};
use crate::config::BridgeConfig;
use crate::config::{DEFAULT_BACKFILL_BATCH_SIZE, DEFAULT_BACKFILL_PLANNER_INTERVAL_SECS};
use crate::health::RuntimeHealthState;
use crate::nostr_consumer::WebSocketRelayConnection;
use crate::pipeline::{
    AccountLink, AccountStore, AssetManifestRecord, BridgePipeline, HttpBlobFetcher,
    PublishJobEnvelope, QueueDecision, RecordMapping, RecordStore,
};
use crate::publisher::PdsClient;
use crate::video_service::VideoServiceUploader;

pub type SharedConnection = Arc<Mutex<PgConnection>>;

const DEFAULT_PUBLISH_JOB_LEASE_SECS: i64 = 120;
const DEFAULT_LIVE_WORKER_INTERVAL_MILLIS: u64 = 250;
const DEFAULT_BACKFILL_WORKER_INTERVAL_MILLIS: u64 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerRunResult {
    Idle,
    Completed {
        nostr_event_id: String,
    },
    Failed {
        nostr_event_id: String,
        error: String,
    },
}

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

/// Resolves a per-account PDS access token (by DID) from the bridge database so
/// repo writes authenticate as the account, as rsky-pds requires.
pub struct DbSessionProvider {
    connection: SharedConnection,
}

impl DbSessionProvider {
    pub fn new(connection: SharedConnection) -> Self {
        Self { connection }
    }
}

#[async_trait::async_trait]
impl crate::publisher::SessionProvider for DbSessionProvider {
    async fn access_token(&self, did: &str) -> Result<Option<String>> {
        let mut connection = self.connection.lock().unwrap();
        get_account_pds_access_jwt_by_did(&mut connection, did)
    }

    async fn refresh_token(&self, did: &str) -> Result<Option<String>> {
        let mut connection = self.connection.lock().unwrap();
        get_account_pds_refresh_jwt_by_did(&mut connection, did)
    }

    async fn store_session(&self, did: &str, access_jwt: &str, refresh_jwt: &str) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        store_account_pds_session_by_did(&mut connection, did, access_jwt, refresh_jwt)
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

fn event_timestamp(created_at: i64) -> Result<DateTime<Utc>> {
    DateTime::<Utc>::from_timestamp(created_at, 0).context("relay event timestamp is out of range")
}

fn new_publish_job<'a>(
    envelope: &'a PublishJobEnvelope,
    source: PublishJobSource,
) -> Result<NewPublishJob<'a>> {
    Ok(NewPublishJob {
        nostr_event_id: &envelope.nostr_event_id,
        nostr_pubkey: &envelope.nostr_pubkey,
        event_created_at: event_timestamp(envelope.event_created_at)?,
        event_payload: envelope.event_payload.clone(),
        job_source: source.as_str(),
        state: PublishState::Pending.as_str(),
    })
}

fn delete_execution_envelope(tombstone_job: &PublishJobEnvelope) -> Result<PublishJobEnvelope> {
    let event: NostrEvent = serde_json::from_value(tombstone_job.event_payload.clone())
        .context("failed to deserialize delete event payload")?;
    Ok(PublishJobEnvelope {
        nostr_event_id: event.id.clone(),
        nostr_pubkey: event.pubkey,
        event_created_at: event.created_at,
        event_payload: tombstone_job.event_payload.clone(),
    })
}

fn publish_job_envelope(job: &PublishJob) -> PublishJobEnvelope {
    PublishJobEnvelope {
        nostr_event_id: job.nostr_event_id.clone(),
        nostr_pubkey: job.nostr_pubkey.clone(),
        event_created_at: job.event_created_at.timestamp(),
        event_payload: job.event_payload.clone(),
    }
}

pub async fn enqueue_live_event<A, R, F, U, P>(
    connection: &SharedConnection,
    relay_source_name: &str,
    pipeline: &BridgePipeline<A, R, F, U, P>,
    event: &NostrEvent,
) -> Result<()>
where
    A: AccountStore,
    R: RecordStore,
    F: crate::pipeline::BlobFetcher,
    U: crate::pipeline::BlobUploader,
    P: crate::pipeline::PdsPublisher,
{
    let decision = pipeline.prepare_publish_job(event).await?;

    {
        let mut conn = connection.lock().unwrap();
        match decision {
            QueueDecision::Enqueue(job) => {
                let queued = new_publish_job(&job, PublishJobSource::Live)?;
                enqueue_publish_job(&mut conn, &queued)?;
            }
            QueueDecision::Cancel {
                target_nostr_event_id,
                tombstone_job,
            } => {
                let tombstone = new_publish_job(&tombstone_job, PublishJobSource::Live)?;
                cancel_publish_job(&mut conn, &tombstone, Some("live delete replay"))?;

                if get_record_mapping(&mut conn, &target_nostr_event_id)?.is_some() {
                    let delete_job_envelope = delete_execution_envelope(&tombstone_job)?;
                    let delete_job = new_publish_job(&delete_job_envelope, PublishJobSource::Live)?;
                    enqueue_publish_job(&mut conn, &delete_job)?;
                }
            }
            QueueDecision::Skip { .. } => {}
        }
    }

    persist_relay_cursor(connection, relay_source_name, &event.id, event.created_at)
}

/// Outcome of a single REST ingest poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RestIngestOutcome {
    /// Number of new events handed to the publish pipeline this poll.
    pub processed: usize,
    /// True if this poll only seeded the cold-start cursor (no history replay).
    pub cold_start_seeded: bool,
}

/// From a batch of fetched events, pick the ones newer than `cursor`, dedup by
/// id, and return them sorted oldest-first so the per-event cursor advances
/// monotonically (a mid-batch failure then resumes, never skips).
fn select_new_events(events: Vec<NostrEvent>, cursor: Option<i64>) -> Vec<NostrEvent> {
    let mut seen = std::collections::HashSet::new();
    let mut selected: Vec<NostrEvent> = events
        .into_iter()
        .filter(|ev| cursor.map(|c| ev.created_at > c).unwrap_or(true))
        .filter(|ev| seen.insert(ev.id.clone()))
        .collect();
    selected.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    selected
}

/// Run one REST live-ingest poll: fetch recent video events for each kind
/// (newest-first, paginating backward until we reach the stored cursor or hit
/// `max_pages`), then enqueue the genuinely-new ones oldest-first.
///
/// Cold start (no stored cursor): seed the cursor to the newest existing event
/// and process nothing, so we never replay all history as "live" crossposts.
#[allow(clippy::too_many_arguments)]
pub async fn run_rest_ingest_once<A, R, F, U, P>(
    rest: &crate::funnelcake_rest::FunnelcakeRestClient,
    connection: &SharedConnection,
    relay_source_name: &str,
    pipeline: &BridgePipeline<A, R, F, U, P>,
    kinds: &[u64],
    limit: u32,
    max_pages: usize,
) -> Result<RestIngestOutcome>
where
    A: AccountStore,
    R: RecordStore,
    F: crate::pipeline::BlobFetcher,
    U: crate::pipeline::BlobUploader,
    P: crate::pipeline::PdsPublisher,
{
    let cursor = load_relay_cursor(connection, relay_source_name)?;
    let cold_start = cursor.is_none();

    let mut fetched: Vec<NostrEvent> = Vec::new();
    let mut newest: Option<(i64, String)> = None;

    for &kind in kinds {
        let mut before: Option<i64> = None;
        for _ in 0..max_pages.max(1) {
            let page = rest.fetch_video_events(kind, before, limit).await?;
            if page.events.is_empty() {
                break;
            }

            let mut reached_cursor = false;
            for ev in &page.events {
                if newest
                    .as_ref()
                    .map(|(c, _)| ev.created_at > *c)
                    .unwrap_or(true)
                {
                    newest = Some((ev.created_at, ev.id.clone()));
                }
                if let Some(c) = cursor {
                    if ev.created_at <= c {
                        reached_cursor = true;
                    }
                }
            }
            fetched.extend(page.events.iter().cloned());

            // Cold start only needs the newest event to seed; one page per kind.
            if cold_start || reached_cursor || !page.has_more || page.next_cursor.is_none() {
                break;
            }
            before = page.next_cursor;
        }
    }

    if cold_start {
        if let Some((created_at, id)) = &newest {
            persist_relay_cursor(connection, relay_source_name, id, *created_at)?;
        }
        return Ok(RestIngestOutcome {
            processed: 0,
            cold_start_seeded: newest.is_some(),
        });
    }

    let new_events = select_new_events(fetched, cursor);
    let processed = new_events.len();
    for ev in &new_events {
        enqueue_live_event(connection, relay_source_name, pipeline, ev).await?;
    }

    Ok(RestIngestOutcome {
        processed,
        cold_start_seeded: false,
    })
}

pub async fn run_publish_worker_once<A, R, F, U, P>(
    connection: &SharedConnection,
    pipeline: &BridgePipeline<A, R, F, U, P>,
    lane: PublishJobSource,
    worker_name: &str,
) -> Result<WorkerRunResult>
where
    A: AccountStore,
    R: RecordStore,
    F: crate::pipeline::BlobFetcher,
    U: crate::pipeline::BlobUploader,
    P: crate::pipeline::PdsPublisher,
{
    let lease_expires_at = Utc::now() + chrono::Duration::seconds(DEFAULT_PUBLISH_JOB_LEASE_SECS);
    let job = {
        let mut conn = connection.lock().unwrap();
        match lane {
            PublishJobSource::Live => {
                claim_next_live_job(&mut conn, worker_name, lease_expires_at)?
            }
            PublishJobSource::Backfill => {
                claim_next_backfill_job(&mut conn, worker_name, lease_expires_at)?
            }
        }
    };

    let Some(job) = job else {
        return Ok(WorkerRunResult::Idle);
    };

    let envelope = publish_job_envelope(&job);
    match pipeline.execute_publish_job(&envelope).await {
        Ok(_) => {
            let mut conn = connection.lock().unwrap();
            mark_publish_job_completed(&mut conn, &job.nostr_event_id)?;
            Ok(WorkerRunResult::Completed {
                nostr_event_id: job.nostr_event_id,
            })
        }
        Err(error) => {
            let error_message = format!("{error:#}");
            let mut conn = connection.lock().unwrap();
            mark_publish_job_failed(&mut conn, &job.nostr_event_id, &error_message)?;
            Ok(WorkerRunResult::Failed {
                nostr_event_id: job.nostr_event_id,
                error: error_message,
            })
        }
    }
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

async fn publish_worker_loop<A, R, F, U, P>(
    lane: PublishJobSource,
    connection: SharedConnection,
    pipeline: Arc<BridgePipeline<A, R, F, U, P>>,
    health: RuntimeHealthState,
) where
    A: AccountStore + 'static,
    R: RecordStore + 'static,
    F: crate::pipeline::BlobFetcher + 'static,
    U: crate::pipeline::BlobUploader + 'static,
    P: crate::pipeline::PdsPublisher + 'static,
{
    let interval_millis = match lane {
        PublishJobSource::Live => DEFAULT_LIVE_WORKER_INTERVAL_MILLIS,
        PublishJobSource::Backfill => DEFAULT_BACKFILL_WORKER_INTERVAL_MILLIS,
    };
    let worker_name = format!("{}-worker-{}", lane.as_str(), std::process::id());
    let mut ticker = tokio::time::interval(Duration::from_millis(interval_millis));

    loop {
        ticker.tick().await;
        match run_publish_worker_once(&connection, pipeline.as_ref(), lane, &worker_name).await {
            Ok(WorkerRunResult::Idle) | Ok(WorkerRunResult::Completed { .. }) => {}
            Ok(WorkerRunResult::Failed {
                nostr_event_id,
                error,
            }) => {
                tracing::warn!(
                    lane = %lane,
                    nostr_event_id = %nostr_event_id,
                    error = %error,
                    "publish worker job failed"
                );
                health.record_processing_failure(error);
            }
            Err(error) => {
                tracing::warn!(
                    lane = %lane,
                    error = %error,
                    "publish worker iteration failed"
                );
                health.record_runtime_failure(error.to_string());
                tokio::time::sleep(health.next_retry_delay()).await;
            }
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
    let session_provider: std::sync::Arc<dyn crate::publisher::SessionProvider> =
        std::sync::Arc::new(DbSessionProvider::new(connection.clone()));
    let pds_client_for_blobs =
        PdsClient::new(config.pds_url.clone(), config.pds_auth_token.clone())
            .with_session_provider(session_provider.clone());
    let blob_uploader: Box<dyn crate::pipeline::BlobUploader> = if config.video_service_enabled {
        tracing::info!(
            video_service_url = %config.video_service_url,
            "video uploads will be routed through video transcoding service"
        );
        Box::new(VideoServiceUploader::new(
            pds_client_for_blobs,
            config.pds_url.clone(),
            config.video_service_url.clone(),
            Duration::from_secs(config.video_service_poll_timeout_secs),
            Duration::from_millis(config.video_service_poll_interval_ms),
        ))
    } else {
        Box::new(pds_client_for_blobs)
    };
    let pds_publisher = PdsClient::new(config.pds_url.clone(), config.pds_auth_token.clone())
        .with_session_provider(session_provider.clone());
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
    let planner_health = health.clone();
    tokio::spawn(async move {
        let mut ticker =
            tokio::time::interval(Duration::from_secs(DEFAULT_BACKFILL_PLANNER_INTERVAL_SECS));
        loop {
            ticker.tick().await;
            if let Err(error) = backfill_planner.run_once().await {
                tracing::warn!(error = %error, "backfill planner run failed");
                planner_health.record_runtime_failure(error.to_string());
            }
        }
    });
    tokio::spawn(publish_worker_loop(
        PublishJobSource::Live,
        connection.clone(),
        pipeline.clone(),
        health.clone(),
    ));
    tokio::spawn(publish_worker_loop(
        PublishJobSource::Backfill,
        connection.clone(),
        pipeline.clone(),
        health.clone(),
    ));

    // Live ingest: poll the Funnelcake REST API for new video events instead of
    // holding a WebSocket firehose. The REST endpoint returns the same full,
    // signed Nostr events; we reuse the publish pipeline + ingest_offsets cursor.
    // (Author-history backfill still uses the WS path via BackfillPlanner.)
    let rest = crate::funnelcake_rest::FunnelcakeRestClient::new(config.relay_rest_url.clone());
    const REST_INGEST_KINDS: [u64; 2] = [34236, 34235];
    const REST_INGEST_LIMIT: u32 = 100;
    const REST_INGEST_MAX_PAGES: usize = 5;

    tracing::info!(
        rest_url = %config.relay_rest_url,
        source = %config.relay_source_name,
        poll_interval_secs = config.relay_poll_interval_secs,
        "starting REST live-ingest poll loop"
    );

    let mut ticker =
        tokio::time::interval(Duration::from_secs(config.relay_poll_interval_secs.max(1)));
    loop {
        ticker.tick().await;
        match run_rest_ingest_once(
            &rest,
            &connection,
            &config.relay_source_name,
            pipeline.as_ref(),
            &REST_INGEST_KINDS,
            REST_INGEST_LIMIT,
            REST_INGEST_MAX_PAGES,
        )
        .await
        {
            Ok(outcome) => {
                if outcome.cold_start_seeded {
                    tracing::info!(
                        source = %config.relay_source_name,
                        "seeded REST ingest cursor (cold start); no history replayed"
                    );
                } else if outcome.processed > 0 {
                    tracing::info!(
                        processed = outcome.processed,
                        "REST ingest enqueued new live events"
                    );
                }
                health.record_success();
            }
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    rest_url = %config.relay_rest_url,
                    "REST ingest poll failed; will retry next tick"
                );
                health.record_runtime_failure(error.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event(id: &str, created_at: i64) -> NostrEvent {
        NostrEvent {
            id: id.to_string(),
            pubkey: "pubkey".to_string(),
            created_at,
            kind: 34236,
            tags: vec![],
            content: String::new(),
            sig: "sig".to_string(),
        }
    }

    #[test]
    fn select_new_events_filters_dedups_and_sorts_oldest_first() {
        let events = vec![
            sample_event("c", 30),
            sample_event("a", 10),
            sample_event("b", 20),
            sample_event("a", 10), // duplicate id
            sample_event("old", 5),
        ];
        // cursor=10 keeps strictly-newer events; 10 and 5 are excluded.
        let out = select_new_events(events, Some(10));
        let ids: Vec<&str> = out.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "c"]);
    }

    #[test]
    fn select_new_events_without_cursor_keeps_all_sorted() {
        let out = select_new_events(
            vec![
                sample_event("c", 30),
                sample_event("a", 10),
                sample_event("b", 20),
            ],
            None,
        );
        let ids: Vec<&str> = out.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }
}
