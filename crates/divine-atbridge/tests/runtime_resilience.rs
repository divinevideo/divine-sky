use std::collections::VecDeque;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use divine_atbridge::nostr_consumer::{NostrConsumer, RelayConnection};
use divine_atbridge::pipeline::{
    AccountLink, AccountStore, BlobFetcher, BlobUploader, BridgePipeline, PdsPublisher,
    PublishedRecord, RecordMapping, RecordStore,
};
use divine_atbridge::run_bridge_session;
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
    async fn put_record(
        &self,
        _did: &str,
        _collection: &str,
        rkey: &str,
        _record: &serde_json::Value,
    ) -> Result<String> {
        if *self.fail_first_write.lock().unwrap() {
            *self.fail_first_write.lock().unwrap() = false;
            return Err(anyhow!("synthetic PDS failure"));
        }

        self.published.lock().unwrap().push(rkey.to_string());
        Ok(format!("at://did:plc:test/app.bsky.actor.profile/{rkey}"))
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

    assert!(result.is_ok(), "processing failure should not terminate the session");
    assert_eq!(consumer.last_seen_timestamp, Some(1_700_000_021));
}
