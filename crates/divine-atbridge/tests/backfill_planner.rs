use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use diesel::Connection;
use diesel::PgConnection;
use diesel::RunQueryDsl;
use diesel::{sql_types, QueryableByName};
use divine_atbridge::backfill_planner::{BackfillPlanner, BackfillRelayConnector};
use divine_atbridge::nostr_consumer::RelayConnection;
use divine_atbridge::pipeline::{
    AccountLink, AccountStore, BlobFetcher, BlobUploader, BridgePipeline, PdsPublisher,
    RecordMapping, RecordStore,
};
use divine_bridge_types::{BlobRef, NostrEvent, PublishJobSource};
use secp256k1::rand::rngs::OsRng;
use secp256k1::{Keypair, Secp256k1};
use sha2::{Digest, Sha256};

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

fn make_signed_event_with_key(
    keypair: &Keypair,
    kind: u64,
    created_at: i64,
    content: &str,
    tags: Vec<Vec<String>>,
) -> NostrEvent {
    let secp = Secp256k1::new();
    let (xonly, _) = keypair.x_only_public_key();
    let pubkey_hex = hex::encode(xonly.serialize());

    let canonical = serde_json::json!([0, pubkey_hex, created_at, kind, tags, content]);
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

struct StubAccountStore {
    link: AccountLink,
}

#[async_trait]
impl AccountStore for StubAccountStore {
    async fn get_account_link(&self, nostr_pubkey: &str) -> Result<Option<AccountLink>> {
        if nostr_pubkey == self.link.nostr_pubkey {
            Ok(Some(self.link.clone()))
        } else {
            Ok(None)
        }
    }
}

struct StubRecordStore;

#[async_trait]
impl RecordStore for StubRecordStore {
    async fn is_event_processed(&self, _event_id: &str) -> Result<bool> {
        Ok(false)
    }

    async fn save_record_mapping(&self, _mapping: RecordMapping) -> Result<()> {
        Ok(())
    }

    async fn get_mapping_by_nostr_id(&self, _event_id: &str) -> Result<Option<RecordMapping>> {
        Ok(None)
    }

    async fn mark_deleted(&self, _event_id: &str) -> Result<()> {
        Ok(())
    }
}

struct PanicBlobFetcher;

#[async_trait]
impl BlobFetcher for PanicBlobFetcher {
    async fn fetch_blob(&self, _url: &str) -> Result<(Vec<u8>, String)> {
        panic!("backfill planner should only prepare jobs, not fetch blobs");
    }
}

struct PanicBlobUploader;

#[async_trait]
impl BlobUploader for PanicBlobUploader {
    async fn upload_blob(&self, _data: &[u8], _mime_type: &str) -> Result<BlobRef> {
        panic!("backfill planner should only prepare jobs, not upload blobs");
    }
}

struct PanicPublisher;

#[async_trait]
impl PdsPublisher for PanicPublisher {
    async fn create_record(
        &self,
        _did: &str,
        _collection: &str,
        _record: &serde_json::Value,
    ) -> Result<String> {
        panic!("backfill planner should not publish records");
    }

    async fn put_record(
        &self,
        _did: &str,
        _collection: &str,
        _rkey: &str,
        _record: &serde_json::Value,
    ) -> Result<String> {
        panic!("backfill planner should not put records");
    }

    async fn delete_record(&self, _did: &str, _collection: &str, _rkey: &str) -> Result<()> {
        panic!("backfill planner should not delete records");
    }
}

struct MockRelayConnection {
    outgoing: Vec<String>,
    incoming: VecDeque<String>,
}

impl MockRelayConnection {
    fn new(messages: Vec<String>) -> Self {
        Self {
            outgoing: Vec::new(),
            incoming: VecDeque::from(messages),
        }
    }
}

#[async_trait]
impl RelayConnection for MockRelayConnection {
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

#[derive(Clone)]
struct MockRelayConnector {
    messages: Vec<String>,
}

#[async_trait]
impl BackfillRelayConnector for MockRelayConnector {
    type Connection = MockRelayConnection;

    async fn connect(&self, _relay_url: &str) -> Result<Self::Connection> {
        Ok(MockRelayConnection::new(self.messages.clone()))
    }
}

#[derive(Debug, QueryableByName)]
struct PublishJobRow {
    #[diesel(sql_type = sql_types::Text)]
    nostr_event_id: String,
    #[diesel(sql_type = sql_types::Text)]
    state: String,
    #[diesel(sql_type = sql_types::Text)]
    job_source: String,
}

#[derive(Debug, QueryableByName)]
struct AccountBackfillStateRow {
    #[diesel(sql_type = sql_types::Text)]
    publish_backfill_state: String,
    #[diesel(sql_type = sql_types::Nullable<sql_types::Timestamptz>)]
    publish_backfill_completed_at: Option<DateTime<Utc>>,
}

#[test]
fn backfill_planner_replays_history_oldest_first_and_is_idempotent() {
    let _guard = test_db_lock().lock().unwrap();
    let database_url = test_database_url();
    reset_database(&database_url);

    let mut conn =
        PgConnection::establish(&database_url).expect("test database should be reachable");
    let secp = Secp256k1::new();
    let keypair = Keypair::new(&secp, &mut OsRng);
    let pubkey = hex::encode(keypair.x_only_public_key().0.serialize());
    insert_ready_account(
        &mut conn,
        &pubkey,
        "did:plc:planner",
        "planner.divine.video",
    );
    drop(conn);

    let publish_old = make_signed_event_with_key(
        &keypair,
        34235,
        100,
        "old publish",
        vec![vec!["d".to_string(), "old".to_string()]],
    );
    let publish_new = make_signed_event_with_key(
        &keypair,
        34235,
        200,
        "new publish",
        vec![vec!["d".to_string(), "new".to_string()]],
    );
    let delete_old = make_signed_event_with_key(
        &keypair,
        5,
        150,
        "",
        vec![vec!["e".to_string(), publish_old.id.clone()]],
    );

    let relay_messages = vec![
        serde_json::json!(["EVENT", "sub-1", publish_new]).to_string(),
        serde_json::json!(["EVENT", "sub-1", publish_old]).to_string(),
        serde_json::json!(["EVENT", "sub-1", delete_old]).to_string(),
        serde_json::json!(["EOSE", "sub-1"]).to_string(),
    ];

    let pipeline = Arc::new(BridgePipeline::new(
        StubAccountStore {
            link: AccountLink {
                nostr_pubkey: pubkey.clone(),
                did: "did:plc:planner".to_string(),
                opted_in: true,
            },
        },
        StubRecordStore,
        PanicBlobFetcher,
        PanicBlobUploader,
        PanicPublisher,
    ));

    let shared_conn = Arc::new(Mutex::new(
        PgConnection::establish(&database_url).expect("test database should be reachable"),
    ));
    let planner = BackfillPlanner::new(
        "wss://relay.example.com".to_string(),
        shared_conn.clone(),
        pipeline,
        MockRelayConnector {
            messages: relay_messages,
        },
        10,
    );

    let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
    runtime
        .block_on(planner.run_once())
        .expect("planner run should succeed");
    runtime
        .block_on(planner.run_once())
        .expect("planner rerun should stay idempotent");

    let mut verify =
        PgConnection::establish(&database_url).expect("test database should be reachable");
    let jobs: Vec<PublishJobRow> = diesel::sql_query(
        "SELECT nostr_event_id, state, job_source
         FROM publish_jobs
         ORDER BY nostr_event_id ASC",
    )
    .load::<PublishJobRow>(&mut verify)
    .expect("jobs should load");

    assert_eq!(jobs.len(), 2, "planner should not duplicate jobs on rerun");
    assert!(
        jobs.iter().any(|row| {
            row.nostr_event_id == publish_old.id
                && row.state == "skipped"
                && row.job_source == PublishJobSource::Backfill.as_str()
        }),
        "old publish should be skipped by historical delete replay"
    );
    assert!(
        jobs.iter().any(|row| {
            row.nostr_event_id == publish_new.id
                && row.state == "pending"
                && row.job_source == PublishJobSource::Backfill.as_str()
        }),
        "new publish should remain pending backfill work"
    );

    let account_state: AccountBackfillStateRow = diesel::sql_query(
        "SELECT publish_backfill_state, publish_backfill_completed_at
         FROM account_links
         WHERE nostr_pubkey = $1",
    )
    .bind::<diesel::sql_types::Text, _>(&pubkey)
    .get_result(&mut verify)
    .expect("account state should load");

    assert_eq!(account_state.publish_backfill_state, "completed");
    assert!(
        account_state.publish_backfill_completed_at.is_some(),
        "planner should mark backlog complete only after EOSE"
    );
}
