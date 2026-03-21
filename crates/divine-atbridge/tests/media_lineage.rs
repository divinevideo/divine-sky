use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;
use divine_atbridge::pipeline::{
    AccountLink, AccountStore, AssetManifestRecord, BlobFetcher, BlobUploader, BridgePipeline,
    PdsPublisher, ProcessResult, PublishedRecord, RecordMapping, RecordStore,
};
use divine_bridge_types::{BlobRef, NostrEvent, RecordStatus};
use secp256k1::rand::rngs::OsRng;
use secp256k1::{Keypair, Secp256k1};
use sha2::{Digest, Sha256};

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

fn make_video_event(url: &str, sha256: &str) -> NostrEvent {
    make_signed_event(
        34235,
        "hash lineage",
        vec![
            vec!["title".into(), "Hash Lineage".into()],
            vec!["url".into(), url.into()],
            vec!["x".into(), sha256.into()],
            vec!["d".into(), "hash-lineage".into()],
        ],
    )
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
    saved: Mutex<Vec<RecordMapping>>,
    manifests: Mutex<Vec<AssetManifestRecord>>,
    statuses: Mutex<Vec<(String, Option<String>, RecordStatus)>>,
}

#[async_trait]
impl RecordStore for TrackingRecordStore {
    async fn is_event_processed(&self, _event_id: &str) -> Result<bool> {
        Ok(false)
    }

    async fn save_record_mapping(&self, mapping: RecordMapping) -> Result<()> {
        self.saved.lock().unwrap().push(mapping);
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

struct StaticBlobFetcher {
    bytes: Vec<u8>,
    mime_type: String,
}

#[async_trait]
impl BlobFetcher for StaticBlobFetcher {
    async fn fetch_blob(&self, _url: &str) -> Result<(Vec<u8>, String)> {
        Ok((self.bytes.clone(), self.mime_type.clone()))
    }
}

struct StaticBlobUploader;

#[async_trait]
impl BlobUploader for StaticBlobUploader {
    async fn upload_blob(&self, data: &[u8], mime_type: &str) -> Result<BlobRef> {
        Ok(BlobRef::new(
            format!("bafkrei{}", hex::encode(&data[..4.min(data.len())])),
            mime_type.to_string(),
            data.len() as u64,
        ))
    }
}

struct TrackingPublisher;

#[async_trait]
impl PdsPublisher for TrackingPublisher {
    async fn create_record(
        &self,
        did: &str,
        collection: &str,
        _record: &serde_json::Value,
    ) -> Result<String> {
        Ok(format!("at://{did}/{collection}/3lineagemedia"))
    }

    async fn create_record_with_meta(
        &self,
        did: &str,
        collection: &str,
        record: &serde_json::Value,
    ) -> Result<PublishedRecord> {
        Ok(PublishedRecord {
            at_uri: self.create_record(did, collection, record).await?,
            rkey: "3lineagemedia".to_string(),
            cid: Some("bafyrecordcid123".to_string()),
        })
    }

    async fn put_record(
        &self,
        did: &str,
        collection: &str,
        rkey: &str,
        _record: &serde_json::Value,
    ) -> Result<String> {
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
            cid: Some("bafyrecordcid123".to_string()),
        })
    }

    async fn delete_record(&self, _did: &str, _collection: &str, _rkey: &str) -> Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn media_lineage_persists_manifest_and_publish_status() {
    let bytes = b"verified-video-payload".to_vec();
    let sha256 = hex::encode(Sha256::digest(&bytes));
    let event = make_video_event("https://blossom.example/video.mp4", &sha256);

    let pipeline = BridgePipeline::new(
        StaticAccountStore {
            link: AccountLink {
                nostr_pubkey: event.pubkey.clone(),
                did: "did:plc:media".to_string(),
                opted_in: true,
            },
        },
        TrackingRecordStore::default(),
        StaticBlobFetcher {
            bytes: bytes.clone(),
            mime_type: "video/mp4".to_string(),
        },
        StaticBlobUploader,
        TrackingPublisher,
    );

    let result = pipeline.process_event(&event).await;
    match result {
        ProcessResult::Published { at_uri, rkey } => {
            assert!(at_uri.contains("did:plc:media"));
            assert_eq!(rkey, "3lineagemedia");
        }
        other => panic!("expected Published, got {other:?}"),
    }

    let manifests = pipeline.record_store.manifests.lock().unwrap();
    assert_eq!(manifests.len(), 1);
    assert_eq!(manifests[0].source_sha256, sha256);
    assert_eq!(
        manifests[0].blossom_url.as_deref(),
        Some("https://blossom.example/video.mp4")
    );
    assert_eq!(manifests[0].mime, "video/mp4");
    assert_eq!(manifests[0].bytes, bytes.len() as u64);
    assert!(!manifests[0].is_derivative);
    drop(manifests);

    let statuses = pipeline.record_store.statuses.lock().unwrap();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].0, event.id);
    assert_eq!(statuses[0].1.as_deref(), Some("bafyrecordcid123"));
    assert_eq!(statuses[0].2, RecordStatus::Published);
}

#[tokio::test]
async fn media_lineage_rejects_sha256_mismatch() {
    let bytes = b"actual-video".to_vec();
    let event = make_video_event("https://blossom.example/video.mp4", &"ab".repeat(32));

    let pipeline = BridgePipeline::new(
        StaticAccountStore {
            link: AccountLink {
                nostr_pubkey: event.pubkey.clone(),
                did: "did:plc:media".to_string(),
                opted_in: true,
            },
        },
        TrackingRecordStore::default(),
        StaticBlobFetcher {
            bytes,
            mime_type: "video/mp4".to_string(),
        },
        StaticBlobUploader,
        TrackingPublisher,
    );

    let result = pipeline.process_event(&event).await;
    match result {
        ProcessResult::Error { message } => {
            assert!(message.contains("SHA-256 mismatch"), "got: {message}");
        }
        other => panic!("expected Error, got {other:?}"),
    }

    assert!(pipeline.record_store.manifests.lock().unwrap().is_empty());
    assert!(pipeline.record_store.statuses.lock().unwrap().is_empty());
}
