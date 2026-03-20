use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use divine_atbridge::nostr_consumer::{NostrConsumer, RelayConnection};
use divine_atbridge::pipeline::{
    AccountLink, AccountStore, AssetManifestRecord, BridgePipeline, HttpBlobFetcher, ProcessResult,
    RecordMapping, RecordStore,
};
use divine_atbridge::publisher::PdsClient;
use divine_atbridge::run_bridge_session;
use divine_bridge_types::{NostrEvent, RecordStatus};
use secp256k1::rand::rngs::OsRng;
use secp256k1::{Keypair, Secp256k1};
use sha2::{Digest, Sha256};

fn make_signed_event(kind: u64, content: &str, tags: Vec<Vec<String>>) -> NostrEvent {
    let secp = Secp256k1::new();
    let keypair = Keypair::new(&secp, &mut OsRng);
    let (xonly, _) = keypair.x_only_public_key();
    let pubkey_hex = hex::encode(xonly.serialize());
    let created_at: i64 = 1_700_000_001;

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

fn make_video_event(url: &str, sha256: &str) -> NostrEvent {
    make_signed_event(
        34235,
        "publish integration",
        vec![
            vec!["title".into(), "Integration".into()],
            vec!["url".into(), url.into()],
            vec!["x".into(), sha256.into()],
            vec!["d".into(), "integration-video".into()],
        ],
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
    manifests: Mutex<Vec<AssetManifestRecord>>,
    statuses: Mutex<Vec<(String, Option<String>, RecordStatus)>>,
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

    async fn save_asset_manifest(&self, entry: AssetManifestRecord) -> Result<()> {
        self.manifests.lock().unwrap().push(entry);
        Ok(())
    }

    async fn update_record_mapping_status(
        &self,
        event_id: &str,
        cid: Option<&str>,
        status: RecordStatus,
    ) -> Result<()> {
        self.statuses
            .lock()
            .unwrap()
            .push((event_id.to_string(), cid.map(str::to_string), status));
        Ok(())
    }
}

#[tokio::test]
async fn publish_path_integration_processes_video_event_through_http_collaborators() {
    let mut blossom_server = mockito::Server::new_async().await;
    let video_bytes = b"publish-path-video".to_vec();
    let source_sha256 = hex::encode(Sha256::digest(&video_bytes));
    let blossom_path = "/blob/integration-video.mp4";

    let blossom_mock = blossom_server
        .mock("GET", blossom_path)
        .with_status(200)
        .with_header("content-type", "video/mp4")
        .with_body(video_bytes.clone())
        .create_async()
        .await;

    let mut pds_server = mockito::Server::new_async().await;
    let upload_mock = pds_server
        .mock("POST", "/xrpc/com.atproto.repo.uploadBlob")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "blob": {
                    "$type": "blob",
                    "ref": {"$link": "bafyreiblobintegration"},
                    "mimeType": "video/mp4",
                    "size": video_bytes.len()
                }
            })
            .to_string(),
        )
        .create_async()
        .await;

    let put_mock = pds_server
        .mock("POST", "/xrpc/com.atproto.repo.putRecord")
        .match_body(mockito::Matcher::Regex("integration-video".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "uri": "at://did:plc:integration/app.bsky.feed.post/integration-video",
                "cid": "bafyrecordintegration"
            })
            .to_string(),
        )
        .create_async()
        .await;

    let event = make_video_event(
        &format!("{}{}", blossom_server.url(), blossom_path),
        &source_sha256,
    );
    let raw = serde_json::json!(["EVENT", "sub-1", event.clone()]).to_string();
    let event_id = event.id.clone();

    let pipeline = BridgePipeline::new(
        StaticAccountStore {
            link: AccountLink {
                nostr_pubkey: event.pubkey.clone(),
                did: "did:plc:integration".to_string(),
                opted_in: true,
            },
        },
        TrackingRecordStore::default(),
        HttpBlobFetcher::new(Duration::from_secs(5)).unwrap(),
        PdsClient::new(pds_server.url(), "integration-token"),
        PdsClient::new(pds_server.url(), "integration-token"),
    );
    let mut consumer = NostrConsumer::new("wss://relay.example".to_string());
    let mut connection = MockConnection::new(vec![raw]);

    run_bridge_session(&mut consumer, &mut connection, &pipeline)
        .await
        .unwrap();

    blossom_mock.assert_async().await;
    upload_mock.assert_async().await;
    put_mock.assert_async().await;

    assert_eq!(consumer.last_seen_timestamp, Some(event.created_at));
    assert_eq!(pipeline.record_store.manifests.lock().unwrap().len(), 1);
    assert_eq!(
        pipeline.record_store.statuses.lock().unwrap()[0],
        (
            event_id,
            Some("bafyrecordintegration".to_string()),
            RecordStatus::Published,
        )
    );
    match &pipeline.process_event(&event).await {
        ProcessResult::Published { .. }
        | ProcessResult::Skipped { .. }
        | ProcessResult::Error { .. }
        | ProcessResult::Deleted { .. }
        | ProcessResult::ProfileSynced { .. } => {}
    }
}
