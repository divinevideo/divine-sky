use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use diesel::Connection;
use diesel::PgConnection;
use diesel::RunQueryDsl;
use divine_atbridge::nostr_consumer::{NostrConsumer, RelayConnection};
use divine_atbridge::pipeline::{
    AccountLink, AccountStore, BlobFetcher, BlobUploader, BridgePipeline, PdsPublisher,
    PublishedRecord, RecordMapping, RecordStore,
};
use divine_atbridge::run_bridge_session;
use divine_atbridge::runtime::{
    enqueue_live_event, run_publish_worker_once, DbAccountStore, DbRecordStore, SharedConnection,
    WorkerRunResult,
};
use divine_bridge_db::models::NewPublishJob;
use divine_bridge_db::{
    enqueue_publish_job, get_ingest_offset, get_publish_job, get_record_mapping,
};
use divine_bridge_types::{BlobRef, NostrEvent, RecordStatus};
use secp256k1::rand::rngs::OsRng;
use secp256k1::{Keypair, Secp256k1};
use serde_json::json;
use sha2::{Digest, Sha256};

fn make_signed_event_with_keypair(
    keypair: &Keypair,
    kind: u64,
    created_at: i64,
    content: &str,
    tags: Vec<Vec<String>>,
) -> NostrEvent {
    let secp = Secp256k1::new();
    let (xonly, _) = keypair.x_only_public_key();
    let pubkey_hex = hex::encode(xonly.serialize());

    let canonical = json!([0, pubkey_hex, created_at, kind, tags, content]);
    let canonical_bytes = serde_json::to_string(&canonical).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(canonical_bytes.as_bytes());
    let id_bytes: [u8; 32] = hasher.finalize().into();
    let id_hex = hex::encode(id_bytes);

    let msg = secp256k1::Message::from_digest(id_bytes);
    let sig = secp.sign_schnorr(&msg, keypair);
    let sig_hex = hex::encode(sig.serialize());

    NostrEvent {
        id: id_hex,
        pubkey: pubkey_hex,
        created_at,
        kind,
        tags,
        content: content.to_string(),
        sig: sig_hex,
    }
}

fn make_profile_event(keypair: &Keypair, created_at: i64, display_name: &str) -> NostrEvent {
    make_signed_event_with_keypair(
        keypair,
        0,
        created_at,
        &json!({
            "display_name": display_name,
            "about": "runtime resilience test"
        })
        .to_string(),
        vec![],
    )
}

fn test_database_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://divine:divine_dev@[::1]:5432/divine_bridge".to_string())
}

fn execute_batch(conn: &mut PgConnection, sql: &str) {
    for statement in sql
        .split(';')
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        diesel::sql_query(statement).execute(conn).unwrap();
    }
}

fn reset_database(database_url: &str) {
    let mut conn =
        PgConnection::establish(database_url).expect("test database should be reachable");
    execute_batch(
        &mut conn,
        include_str!("../../../migrations/001_bridge_tables/down.sql"),
    );
    execute_batch(
        &mut conn,
        include_str!("../../../migrations/001_bridge_tables/up.sql"),
    );
    execute_batch(
        &mut conn,
        include_str!("../../../migrations/004_publish_job_scheduler/up.sql"),
    );
}

fn test_db_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn shared_connection(database_url: &str) -> SharedConnection {
    Arc::new(Mutex::new(
        PgConnection::establish(database_url).expect("test database should be reachable"),
    ))
}

fn insert_ready_account(conn: &mut PgConnection, nostr_pubkey: &str, did: &str, handle: &str) {
    diesel::sql_query(format!(
        "INSERT INTO account_links (
            nostr_pubkey, did, handle, crosspost_enabled, signing_key_id,
            plc_rotation_key_ref, provisioning_state, provisioning_error,
            publish_backfill_state, publish_backfill_started_at,
            publish_backfill_completed_at, publish_backfill_error,
            disabled_at
        ) VALUES (
            '{nostr_pubkey}', '{did}', '{handle}', TRUE, 'signing-{nostr_pubkey}',
            'rotation-{nostr_pubkey}', 'ready', NULL, 'not_started', NULL, NULL, NULL, NULL
        )"
    ))
    .execute(conn)
    .expect("account should insert");
}

fn build_runtime_pipeline(
    connection: SharedConnection,
    publisher: FlakyPublisher,
) -> BridgePipeline<DbAccountStore, DbRecordStore, NoopBlobFetcher, NoopBlobUploader, FlakyPublisher>
{
    BridgePipeline::new(
        DbAccountStore::new(connection.clone()),
        DbRecordStore::new(connection),
        NoopBlobFetcher,
        NoopBlobUploader,
        publisher,
    )
}

struct MockConnection {
    outgoing: Vec<String>,
    incoming: VecDeque<String>,
}

impl MockConnection {
    fn new(messages: Vec<String>) -> Self {
        Self {
            outgoing: Vec::new(),
            incoming: VecDeque::from(messages),
        }
    }
}

#[async_trait]
impl RelayConnection for MockConnection {
    async fn send(&mut self, msg: String) -> Result<()> {
        self.outgoing.push(msg);
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<String>> {
        Ok(self.incoming.pop_front())
    }

    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

struct StaticAccountStore {
    link: AccountLink,
}

#[async_trait]
impl AccountStore for StaticAccountStore {
    async fn get_account_link(&self, nostr_pubkey: &str) -> Result<Option<AccountLink>> {
        if nostr_pubkey == self.link.nostr_pubkey {
            Ok(Some(self.link.clone()))
        } else {
            Ok(None)
        }
    }
}

#[derive(Default)]
struct TrackingRecordStore {
    mappings: Mutex<Vec<RecordMapping>>,
    statuses: Mutex<Vec<(String, RecordStatus)>>,
}

#[async_trait]
impl RecordStore for TrackingRecordStore {
    async fn is_event_processed(&self, _event_id: &str) -> Result<bool> {
        Ok(false)
    }

    async fn save_record_mapping(&self, mapping: RecordMapping) -> Result<()> {
        self.mappings.lock().unwrap().push(mapping);
        Ok(())
    }

    async fn get_mapping_by_nostr_id(&self, _event_id: &str) -> Result<Option<RecordMapping>> {
        Ok(None)
    }

    async fn mark_deleted(&self, _event_id: &str) -> Result<()> {
        Ok(())
    }

    async fn update_record_mapping_status(
        &self,
        event_id: &str,
        _cid: Option<&str>,
        status: RecordStatus,
    ) -> Result<()> {
        self.statuses
            .lock()
            .unwrap()
            .push((event_id.to_string(), status));
        Ok(())
    }
}

struct NoopBlobFetcher;

#[async_trait]
impl BlobFetcher for NoopBlobFetcher {
    async fn fetch_blob(&self, _url: &str) -> Result<(Vec<u8>, String)> {
        Ok((vec![], "application/octet-stream".to_string()))
    }
}

struct NoopBlobUploader;

#[async_trait]
impl BlobUploader for NoopBlobUploader {
    async fn upload_blob(&self, _data: &[u8], _mime_type: &str) -> Result<BlobRef> {
        Ok(BlobRef::new(
            "bafkqaaa".to_string(),
            "application/octet-stream".to_string(),
            0,
        ))
    }
}

#[derive(Default)]
struct FlakyPublisher {
    fail_first_write: Mutex<bool>,
    published: Mutex<Vec<String>>,
}

#[async_trait]
impl PdsPublisher for FlakyPublisher {
    async fn create_record(
        &self,
        _did: &str,
        _collection: &str,
        _record: &serde_json::Value,
    ) -> Result<String> {
        Err(anyhow!("video publish path not used in this test"))
    }

    async fn put_record(
        &self,
        did: &str,
        collection: &str,
        rkey: &str,
        _record: &serde_json::Value,
    ) -> Result<String> {
        if *self.fail_first_write.lock().unwrap() {
            *self.fail_first_write.lock().unwrap() = false;
            return Err(anyhow!("synthetic PDS failure"));
        }

        self.published.lock().unwrap().push(rkey.to_string());
        Ok(format!("at://{did}/{collection}/{rkey}"))
    }

    async fn put_record_with_meta(
        &self,
        did: &str,
        collection: &str,
        rkey: &str,
        record: &serde_json::Value,
    ) -> Result<PublishedRecord> {
        Ok(PublishedRecord {
            at_uri: self.put_record(did, collection, rkey, record).await?,
            rkey: rkey.to_string(),
            cid: Some("bafyrecord".to_string()),
        })
    }

    async fn delete_record(&self, _did: &str, _collection: &str, _rkey: &str) -> Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn run_bridge_session_skips_malformed_frame_and_processes_later_event() {
    let secp = Secp256k1::new();
    let keypair = Keypair::new(&secp, &mut OsRng);
    let event = make_profile_event(&keypair, 1_700_000_010, "Recovered Profile");
    let account_link = AccountLink {
        nostr_pubkey: event.pubkey.clone(),
        did: "did:plc:test".to_string(),
        opted_in: true,
    };

    let record_store = TrackingRecordStore::default();
    let publisher = FlakyPublisher {
        fail_first_write: Mutex::new(false),
        published: Mutex::new(vec![]),
    };
    let pipeline = BridgePipeline::new(
        StaticAccountStore { link: account_link },
        record_store,
        NoopBlobFetcher,
        NoopBlobUploader,
        publisher,
    );
    let mut consumer = NostrConsumer::new("wss://relay.example".to_string());
    let mut connection = MockConnection::new(vec![
        "not-json".to_string(),
        json!(["EVENT", "sub-1", event]).to_string(),
    ]);

    let result = run_bridge_session(&mut consumer, &mut connection, &pipeline).await;

    assert!(result.is_ok(), "malformed frame should be skipped");
    assert_eq!(consumer.last_seen_timestamp, Some(1_700_000_010));
}

#[tokio::test]
async fn run_bridge_session_continues_after_processing_error() {
    let secp = Secp256k1::new();
    let keypair = Keypair::new(&secp, &mut OsRng);
    let first_event = make_profile_event(&keypair, 1_700_000_020, "First Profile");
    let second_event = make_profile_event(&keypair, 1_700_000_021, "Second Profile");
    let account_link = AccountLink {
        nostr_pubkey: first_event.pubkey.clone(),
        did: "did:plc:test".to_string(),
        opted_in: true,
    };

    let publisher = FlakyPublisher {
        fail_first_write: Mutex::new(true),
        published: Mutex::new(vec![]),
    };
    let pipeline = BridgePipeline::new(
        StaticAccountStore { link: account_link },
        TrackingRecordStore::default(),
        NoopBlobFetcher,
        NoopBlobUploader,
        publisher,
    );
    let mut consumer = NostrConsumer::new("wss://relay.example".to_string());
    let mut connection = MockConnection::new(vec![
        json!(["EVENT", "sub-1", first_event]).to_string(),
        json!(["EVENT", "sub-1", second_event]).to_string(),
    ]);

    let result = run_bridge_session(&mut consumer, &mut connection, &pipeline).await;

    assert!(
        result.is_ok(),
        "processing failure should not terminate the session"
    );
    assert_eq!(consumer.last_seen_timestamp, Some(1_700_000_021));
}

#[tokio::test]
async fn runtime_scheduler_persists_cursor_after_enqueue_before_publish_completion() {
    let _guard = test_db_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let database_url = test_database_url();
    reset_database(&database_url);
    let connection = shared_connection(&database_url);

    let secp = Secp256k1::new();
    let keypair = Keypair::new(&secp, &mut OsRng);
    let event = make_profile_event(&keypair, 1_700_100_010, "Queued Before Failure");
    {
        let mut conn = connection.lock().unwrap();
        insert_ready_account(
            &mut conn,
            &event.pubkey,
            "did:plc:runtime-cursor",
            "runtime-cursor.divine.video",
        );
    }

    let pipeline = build_runtime_pipeline(
        connection.clone(),
        FlakyPublisher {
            fail_first_write: Mutex::new(true),
            published: Mutex::new(vec![]),
        },
    );

    enqueue_live_event(&connection, "runtime-test", &pipeline, &event)
        .await
        .expect("ingest should persist the queued job");

    {
        let mut conn = connection.lock().unwrap();
        let offset = get_ingest_offset(&mut conn, "runtime-test")
            .expect("cursor lookup should succeed")
            .expect("cursor should exist after enqueue");
        assert_eq!(offset.last_event_id, event.id);
        assert_eq!(offset.last_created_at.timestamp(), event.created_at);

        let job = get_publish_job(&mut conn, &event.id)
            .expect("job lookup should succeed")
            .expect("job should be queued");
        assert_eq!(job.state, "pending");
    }

    let result = run_publish_worker_once(
        &connection,
        &pipeline,
        divine_bridge_types::PublishJobSource::Live,
        "runtime-live-worker",
    )
    .await
    .expect("worker iteration should finish");

    assert!(matches!(
        result,
        WorkerRunResult::Failed { ref nostr_event_id, .. } if nostr_event_id == &event.id
    ));

    let mut conn = connection.lock().unwrap();
    let job = get_publish_job(&mut conn, &event.id)
        .expect("job lookup should succeed")
        .expect("job should still exist");
    assert_eq!(job.state, "failed");
    let offset = get_ingest_offset(&mut conn, "runtime-test")
        .expect("cursor lookup should succeed")
        .expect("cursor should remain persisted");
    assert_eq!(offset.last_event_id, event.id);
}

#[tokio::test]
async fn runtime_scheduler_runs_live_lane_without_waiting_for_backfill() {
    let _guard = test_db_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let database_url = test_database_url();
    reset_database(&database_url);
    let connection = shared_connection(&database_url);

    let secp = Secp256k1::new();
    let live_keypair = Keypair::new(&secp, &mut OsRng);
    let backfill_keypair = Keypair::new(&secp, &mut OsRng);
    let live_event = make_profile_event(&live_keypair, 1_700_100_020, "Live Lane");
    let backfill_event = make_profile_event(&backfill_keypair, 1_700_099_000, "Backfill Lane");

    {
        let mut conn = connection.lock().unwrap();
        insert_ready_account(
            &mut conn,
            &live_event.pubkey,
            "did:plc:runtime-live",
            "runtime-live.divine.video",
        );
        insert_ready_account(
            &mut conn,
            &backfill_event.pubkey,
            "did:plc:runtime-backfill",
            "runtime-backfill.divine.video",
        );
        enqueue_publish_job(
            &mut conn,
            &NewPublishJob {
                nostr_event_id: &backfill_event.id,
                nostr_pubkey: &backfill_event.pubkey,
                event_created_at: Utc
                    .timestamp_opt(backfill_event.created_at, 0)
                    .single()
                    .expect("backfill timestamp should be valid"),
                event_payload: serde_json::to_value(&backfill_event)
                    .expect("backfill event should serialize"),
                job_source: divine_bridge_types::PublishJobSource::Backfill.as_str(),
                state: "pending",
            },
        )
        .expect("backfill job should enqueue");
    }

    let pipeline = build_runtime_pipeline(
        connection.clone(),
        FlakyPublisher {
            fail_first_write: Mutex::new(false),
            published: Mutex::new(vec![]),
        },
    );

    enqueue_live_event(&connection, "runtime-test", &pipeline, &live_event)
        .await
        .expect("live event should enqueue");

    let live_result = run_publish_worker_once(
        &connection,
        &pipeline,
        divine_bridge_types::PublishJobSource::Live,
        "runtime-live-worker",
    )
    .await
    .expect("live worker should finish");
    assert!(matches!(
        live_result,
        WorkerRunResult::Completed { ref nostr_event_id } if nostr_event_id == &live_event.id
    ));

    {
        let mut conn = connection.lock().unwrap();
        let live_job = get_publish_job(&mut conn, &live_event.id)
            .expect("live job lookup should succeed")
            .expect("live job should exist");
        let backfill_job = get_publish_job(&mut conn, &backfill_event.id)
            .expect("backfill job lookup should succeed")
            .expect("backfill job should exist");
        assert_eq!(live_job.state, "published");
        assert_eq!(backfill_job.state, "pending");
        assert!(get_record_mapping(&mut conn, &live_event.id)
            .expect("live mapping lookup should succeed")
            .is_some());
        assert!(get_record_mapping(&mut conn, &backfill_event.id)
            .expect("backfill mapping lookup should succeed")
            .is_none());
    }

    let backfill_result = run_publish_worker_once(
        &connection,
        &pipeline,
        divine_bridge_types::PublishJobSource::Backfill,
        "runtime-backfill-worker",
    )
    .await
    .expect("backfill worker should finish");
    assert!(matches!(
        backfill_result,
        WorkerRunResult::Completed { ref nostr_event_id } if nostr_event_id == &backfill_event.id
    ));

    let mut conn = connection.lock().unwrap();
    let backfill_job = get_publish_job(&mut conn, &backfill_event.id)
        .expect("backfill job lookup should succeed")
        .expect("backfill job should still exist");
    assert_eq!(backfill_job.state, "published");
    assert!(get_record_mapping(&mut conn, &backfill_event.id)
        .expect("backfill mapping lookup should succeed")
        .is_some());
}

#[tokio::test]
async fn runtime_scheduler_reclaims_expired_worker_leases() {
    let _guard = test_db_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let database_url = test_database_url();
    reset_database(&database_url);
    let connection = shared_connection(&database_url);

    let secp = Secp256k1::new();
    let keypair = Keypair::new(&secp, &mut OsRng);
    let event = make_profile_event(&keypair, 1_700_100_030, "Lease Recovery");

    {
        let mut conn = connection.lock().unwrap();
        insert_ready_account(
            &mut conn,
            &event.pubkey,
            "did:plc:runtime-lease",
            "runtime-lease.divine.video",
        );
        enqueue_publish_job(
            &mut conn,
            &NewPublishJob {
                nostr_event_id: &event.id,
                nostr_pubkey: &event.pubkey,
                event_created_at: Utc
                    .timestamp_opt(event.created_at, 0)
                    .single()
                    .expect("lease timestamp should be valid"),
                event_payload: serde_json::to_value(&event).expect("event should serialize"),
                job_source: divine_bridge_types::PublishJobSource::Live.as_str(),
                state: "pending",
            },
        )
        .expect("job should enqueue");
        diesel::sql_query(format!(
            "UPDATE publish_jobs
             SET state = 'in_progress',
                 lease_owner = 'dead-worker',
                 lease_expires_at = NOW() - INTERVAL '1 second'
             WHERE nostr_event_id = '{}'",
            event.id
        ))
        .execute(&mut *conn)
        .expect("job lease should be forced expired");
    }

    let pipeline = build_runtime_pipeline(
        connection.clone(),
        FlakyPublisher {
            fail_first_write: Mutex::new(false),
            published: Mutex::new(vec![]),
        },
    );

    let result = run_publish_worker_once(
        &connection,
        &pipeline,
        divine_bridge_types::PublishJobSource::Live,
        "runtime-live-worker",
    )
    .await
    .expect("worker should reclaim the expired lease");

    assert!(matches!(
        result,
        WorkerRunResult::Completed { ref nostr_event_id } if nostr_event_id == &event.id
    ));

    let mut conn = connection.lock().unwrap();
    let job = get_publish_job(&mut conn, &event.id)
        .expect("job lookup should succeed")
        .expect("job should still exist");
    assert_eq!(job.state, "published");
}
