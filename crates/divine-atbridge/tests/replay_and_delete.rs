use std::collections::VecDeque;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use divine_atbridge::nostr_consumer::{NostrConsumer, NostrFilter, RelayConnection};
use divine_atbridge::pipeline::{
    AccountLink, AccountStore, BlobFetcher, BlobUploader, BridgePipeline, PdsPublisher,
    ProcessResult, QueueDecision, RecordMapping, RecordStore,
};
use divine_bridge_types::{BlobRef, NostrEvent};
use secp256k1::rand::rngs::OsRng;
use secp256k1::{Keypair, Secp256k1};
use sha2::{Digest, Sha256};

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

fn make_signed_event(kind: u64, content: &str, tags: Vec<Vec<String>>) -> NostrEvent {
    let secp = Secp256k1::new();
    let keypair = Keypair::new(&secp, &mut OsRng);
    let (xonly, _) = keypair.x_only_public_key();
    let pubkey_hex = hex::encode(xonly.serialize());
    let created_at: i64 = 1_700_000_000;

    let canonical = serde_json::json!([0, pubkey_hex, created_at, kind, tags, content]);
    let canonical_bytes = serde_json::to_string(&canonical).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(canonical_bytes.as_bytes());
    let id_bytes: [u8; 32] = hasher.finalize().into();
    let id_hex = hex::encode(id_bytes);

    let msg = secp256k1::Message::from_digest(id_bytes);
    let sig = secp.sign_schnorr(&msg, &keypair);
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

fn make_video_event() -> NostrEvent {
    make_signed_event(
        34235,
        "",
        vec![
            vec!["title".into(), "Replay Test".into()],
            vec!["url".into(), "https://blossom.example/video.mp4".into()],
            vec!["d".into(), "replay-test".into()],
        ],
    )
}

fn make_deletion_event(target_id: &str) -> NostrEvent {
    make_signed_event(5, "", vec![vec!["e".into(), target_id.into()]])
}

struct StubAccountStore {
    links: Vec<AccountLink>,
}

#[async_trait]
impl AccountStore for StubAccountStore {
    async fn get_account_link(&self, nostr_pubkey: &str) -> Result<Option<AccountLink>> {
        Ok(self
            .links
            .iter()
            .find(|link| link.nostr_pubkey == nostr_pubkey)
            .cloned())
    }
}

struct StubRecordStore {
    processed_ids: Vec<String>,
    mappings: Vec<RecordMapping>,
    deleted: Mutex<Vec<String>>,
}

#[async_trait]
impl RecordStore for StubRecordStore {
    async fn is_event_processed(&self, event_id: &str) -> Result<bool> {
        Ok(self.processed_ids.iter().any(|id| id == event_id))
    }

    async fn save_record_mapping(&self, _mapping: RecordMapping) -> Result<()> {
        Ok(())
    }

    async fn get_mapping_by_nostr_id(&self, event_id: &str) -> Result<Option<RecordMapping>> {
        Ok(self
            .mappings
            .iter()
            .find(|mapping| mapping.nostr_event_id == event_id)
            .cloned())
    }

    async fn mark_deleted(&self, event_id: &str) -> Result<()> {
        self.deleted.lock().unwrap().push(event_id.to_string());
        Ok(())
    }
}

struct StubBlobFetcher;

#[async_trait]
impl BlobFetcher for StubBlobFetcher {
    async fn fetch_blob(&self, _url: &str) -> Result<(Vec<u8>, String)> {
        Ok((vec![0xde, 0xad], "video/mp4".to_string()))
    }
}

struct StubBlobUploader;

#[async_trait]
impl BlobUploader for StubBlobUploader {
    async fn upload_blob(&self, _data: &[u8], _mime_type: &str) -> Result<BlobRef> {
        Ok(BlobRef::new(
            "bafkreireplayblob".to_string(),
            "video/mp4".to_string(),
            2,
        ))
    }
}

struct TrackingPublisher {
    deleted: Mutex<Vec<(String, String, String)>>,
}

impl TrackingPublisher {
    fn new() -> Self {
        Self {
            deleted: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl PdsPublisher for TrackingPublisher {
    async fn create_record(
        &self,
        _did: &str,
        _collection: &str,
        _record: &serde_json::Value,
    ) -> Result<String> {
        Ok("at://did:plc:test/app.bsky.feed.post/3replayrecord".to_string())
    }

    async fn put_record(
        &self,
        _did: &str,
        _collection: &str,
        _rkey: &str,
        _record: &serde_json::Value,
    ) -> Result<String> {
        Ok("at://did:plc:test/app.bsky.feed.post/replay".to_string())
    }

    async fn delete_record(&self, did: &str, collection: &str, rkey: &str) -> Result<()> {
        self.deleted.lock().unwrap().push((
            did.to_string(),
            collection.to_string(),
            rkey.to_string(),
        ));
        Ok(())
    }
}

#[tokio::test]
async fn replay_and_delete_prepare_delete_cancels_without_mapping() {
    let delete_event = make_deletion_event("nostr-target-queued");
    let pipeline = BridgePipeline::new(
        StubAccountStore {
            links: vec![AccountLink {
                nostr_pubkey: delete_event.pubkey.clone(),
                did: "did:plc:deleter".to_string(),
                opted_in: true,
            }],
        },
        StubRecordStore {
            processed_ids: vec![],
            mappings: vec![],
            deleted: Mutex::new(Vec::new()),
        },
        StubBlobFetcher,
        StubBlobUploader,
        TrackingPublisher::new(),
    );

    let decision = pipeline
        .prepare_publish_job(&delete_event)
        .await
        .expect("prepare should classify delete as queue cancellation");

    match decision {
        QueueDecision::Cancel {
            target_nostr_event_id,
            tombstone_job,
        } => {
            assert_eq!(target_nostr_event_id, "nostr-target-queued");
            assert_eq!(tombstone_job.nostr_event_id, "nostr-target-queued");
            assert_eq!(tombstone_job.nostr_pubkey, delete_event.pubkey);
            assert_eq!(tombstone_job.event_payload["id"], delete_event.id);
        }
        other => panic!("expected cancel decision, got {other:?}"),
    }
}

#[tokio::test]
async fn replay_and_delete_prepare_delete_cancels_older_backlog_create_before_publish() {
    let create_event = make_video_event();
    let delete_event = make_deletion_event(&create_event.id);

    let pipeline = BridgePipeline::new(
        StubAccountStore {
            links: vec![
                AccountLink {
                    nostr_pubkey: create_event.pubkey.clone(),
                    did: "did:plc:deleter".to_string(),
                    opted_in: true,
                },
                AccountLink {
                    nostr_pubkey: delete_event.pubkey.clone(),
                    did: "did:plc:deleter".to_string(),
                    opted_in: true,
                },
            ],
        },
        StubRecordStore {
            processed_ids: vec![],
            mappings: vec![],
            deleted: Mutex::new(Vec::new()),
        },
        StubBlobFetcher,
        StubBlobUploader,
        TrackingPublisher::new(),
    );

    let queued_create = match pipeline.prepare_publish_job(&create_event).await.unwrap() {
        QueueDecision::Enqueue(job) => job,
        other => panic!("expected enqueue decision, got {other:?}"),
    };

    let delete_decision = pipeline
        .prepare_publish_job(&delete_event)
        .await
        .expect("delete replay should produce cancellation");

    match delete_decision {
        QueueDecision::Cancel {
            target_nostr_event_id,
            tombstone_job,
        } => {
            assert_eq!(target_nostr_event_id, queued_create.nostr_event_id);
            assert_eq!(tombstone_job.nostr_event_id, queued_create.nostr_event_id);
        }
        other => panic!("expected cancel decision, got {other:?}"),
    }
}

#[tokio::test]
async fn replay_and_delete_does_not_advance_cursor_on_failed_callback() {
    let event = make_video_event();
    let raw = serde_json::json!(["EVENT", "sub-1", event]).to_string();
    let mut conn = MockConnection::new(vec![raw]);
    let filter = NostrFilter::nip71_video();
    let mut consumer = NostrConsumer::new("wss://relay.example".to_string());

    let err = consumer
        .subscribe(&mut conn, &filter, |_event| -> Result<()> {
            Err(anyhow!("publish failed"))
        })
        .await
        .expect_err("callback failure should bubble up");

    assert!(
        err.to_string().contains("event processing callback failed"),
        "got: {err:#}"
    );
    assert_eq!(consumer.last_seen_timestamp, None);
}

#[tokio::test]
async fn replay_and_delete_reconnect_uses_last_successful_timestamp() {
    let event = make_video_event();
    let ts = event.created_at;
    let raw = serde_json::json!(["EVENT", "sub-1", event]).to_string();
    let filter = NostrFilter::nip71_video();
    let mut consumer = NostrConsumer::new("wss://relay.example".to_string());

    let mut first_conn = MockConnection::new(vec![raw]);
    consumer
        .subscribe(&mut first_conn, &filter, |_event| -> Result<()> { Ok(()) })
        .await
        .expect("successful callback should advance cursor");

    let mut reconnect = MockConnection::new(vec![]);
    consumer
        .subscribe(&mut reconnect, &filter, |_event| -> Result<()> { Ok(()) })
        .await
        .expect("empty reconnect should still send request");

    let req: serde_json::Value = serde_json::from_str(&reconnect.outgoing[0]).unwrap();
    assert_eq!(req[2]["since"], ts);
}

#[tokio::test]
async fn replay_and_delete_rejects_delete_owner_mismatch() {
    let delete_event = make_deletion_event("nostr-target-1");
    let account_did = "did:plc:deleter".to_string();
    let mapping_did = "did:plc:owner".to_string();

    let pipeline = BridgePipeline::new(
        StubAccountStore {
            links: vec![AccountLink {
                nostr_pubkey: delete_event.pubkey.clone(),
                did: account_did,
                opted_in: true,
            }],
        },
        StubRecordStore {
            processed_ids: vec![],
            mappings: vec![RecordMapping {
                nostr_event_id: "nostr-target-1".to_string(),
                at_uri: "at://did:plc:owner/app.bsky.feed.post/replay-delete".to_string(),
                did: mapping_did,
                collection: "app.bsky.feed.post".to_string(),
                rkey: "replay-delete".to_string(),
                deleted: false,
            }],
            deleted: Mutex::new(Vec::new()),
        },
        StubBlobFetcher,
        StubBlobUploader,
        TrackingPublisher::new(),
    );

    let result = pipeline.process_event(&delete_event).await;

    match result {
        ProcessResult::Skipped { reason } => {
            assert!(reason.contains("does not own"), "got: {reason}");
        }
        other => panic!("expected Skipped, got {other:?}"),
    }

    assert!(pipeline.pds_publisher.deleted.lock().unwrap().is_empty());
    assert!(pipeline.record_store.deleted.lock().unwrap().is_empty());
}

#[tokio::test]
async fn replay_and_delete_uses_mapping_did_for_delete_call() {
    let delete_event = make_deletion_event("nostr-target-2");
    let expected_did = "did:plc:mapped".to_string();

    let pipeline = BridgePipeline::new(
        StubAccountStore {
            links: vec![AccountLink {
                nostr_pubkey: delete_event.pubkey.clone(),
                did: expected_did.clone(),
                opted_in: true,
            }],
        },
        StubRecordStore {
            processed_ids: vec![],
            mappings: vec![RecordMapping {
                nostr_event_id: "nostr-target-2".to_string(),
                at_uri: "at://did:plc:mapped/app.bsky.feed.post/replay-delete".to_string(),
                did: expected_did.clone(),
                collection: "app.bsky.feed.post".to_string(),
                rkey: "replay-delete".to_string(),
                deleted: false,
            }],
            deleted: Mutex::new(Vec::new()),
        },
        StubBlobFetcher,
        StubBlobUploader,
        TrackingPublisher::new(),
    );

    let result = pipeline.process_event(&delete_event).await;

    match result {
        ProcessResult::Deleted { at_uri } => {
            assert!(at_uri.contains(&expected_did));
        }
        other => panic!("expected Deleted, got {other:?}"),
    }

    let deleted = pipeline.pds_publisher.deleted.lock().unwrap();
    assert_eq!(
        deleted[0],
        (
            expected_did,
            "app.bsky.feed.post".to_string(),
            "replay-delete".to_string()
        )
    );
}
