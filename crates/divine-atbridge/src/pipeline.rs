//! Bridge Orchestrator — the main pipeline that wires together all modules.
//!
//! Flow: nostr_consumer → signature verify → translator → blob upload → publisher
//!
//! Uses trait-based abstractions for all external dependencies so the
//! orchestration logic is pure and fully testable with mocks.

use anyhow::{Context, Result};
use async_trait::async_trait;
use divine_bridge_types::{BlobRef, NostrEvent};

use crate::deletion::validate_delete_request;
use crate::signature::verify_nostr_event;
use crate::translator::{derive_rkey, translate_nip71_to_post};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Links a Nostr pubkey to an AT Protocol DID.
#[derive(Debug, Clone)]
pub struct AccountLink {
    pub nostr_pubkey: String,
    pub did: String,
    pub opted_in: bool,
}

/// Maps a bridged Nostr event to its AT Protocol record.
#[derive(Debug, Clone)]
pub struct RecordMapping {
    pub nostr_event_id: String,
    pub at_uri: String,
    pub did: String,
    pub collection: String,
    pub rkey: String,
    pub deleted: bool,
}

/// Result of processing a single Nostr event through the pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessResult {
    Published { at_uri: String, rkey: String },
    Deleted { at_uri: String },
    Skipped { reason: String },
    Error { message: String },
}

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Lookup account linkage between Nostr and ATProto.
#[async_trait]
pub trait AccountStore: Send + Sync {
    async fn get_account_link(&self, nostr_pubkey: &str) -> Result<Option<AccountLink>>;
}

/// Idempotency and record mapping storage.
#[async_trait]
pub trait RecordStore: Send + Sync {
    async fn is_event_processed(&self, event_id: &str) -> Result<bool>;
    async fn save_record_mapping(&self, mapping: RecordMapping) -> Result<()>;
    async fn get_mapping_by_nostr_id(&self, event_id: &str) -> Result<Option<RecordMapping>>;
    async fn mark_deleted(&self, event_id: &str) -> Result<()>;
}

/// Fetch a blob from a Blossom server (or other source).
#[async_trait]
pub trait BlobFetcher: Send + Sync {
    /// Returns (bytes, mime_type).
    async fn fetch_blob(&self, url: &str) -> Result<(Vec<u8>, String)>;
}

/// Upload a blob to a PDS.
#[async_trait]
pub trait BlobUploader: Send + Sync {
    async fn upload_blob(&self, data: &[u8], mime_type: &str) -> Result<BlobRef>;
}

/// Publish / delete records on a PDS.
#[async_trait]
pub trait PdsPublisher: Send + Sync {
    async fn put_record(
        &self,
        did: &str,
        collection: &str,
        rkey: &str,
        record: &serde_json::Value,
    ) -> Result<String>; // returns at_uri

    async fn delete_record(
        &self,
        did: &str,
        collection: &str,
        rkey: &str,
    ) -> Result<()>;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the video URL from the event's `url` tag or `imeta` tag.
fn get_video_url(event: &NostrEvent) -> Option<String> {
    // Try "url" tag first
    for tag in &event.tags {
        if tag.len() >= 2 && tag[0] == "url" {
            return Some(tag[1].clone());
        }
    }
    // Try imeta tag
    for tag in &event.tags {
        if tag.first().map(|s| s.as_str()) == Some("imeta") {
            for entry in &tag[1..] {
                if let Some(val) = entry.strip_prefix("url ") {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

/// Extract the target event ID from a kind-5 deletion event's `e` tag.
fn get_deleted_event_id(event: &NostrEvent) -> Option<&str> {
    event
        .tags
        .iter()
        .find(|t| t.len() >= 2 && t[0] == "e")
        .map(|t| t[1].as_str())
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// The main bridge pipeline that orchestrates event processing.
pub struct BridgePipeline<A, R, F, U, P> {
    pub account_store: A,
    pub record_store: R,
    pub blob_fetcher: F,
    pub blob_uploader: U,
    pub pds_publisher: P,
}

impl<A, R, F, U, P> BridgePipeline<A, R, F, U, P>
where
    A: AccountStore,
    R: RecordStore,
    F: BlobFetcher,
    U: BlobUploader,
    P: PdsPublisher,
{
    pub fn new(
        account_store: A,
        record_store: R,
        blob_fetcher: F,
        blob_uploader: U,
        pds_publisher: P,
    ) -> Self {
        Self {
            account_store,
            record_store,
            blob_fetcher,
            blob_uploader,
            pds_publisher,
        }
    }

    /// Process a single Nostr event through the full bridge pipeline.
    pub async fn process_event(&self, event: &NostrEvent) -> ProcessResult {
        match self.process_event_inner(event).await {
            Ok(result) => result,
            Err(e) => ProcessResult::Error {
                message: format!("{e:#}"),
            },
        }
    }

    async fn process_event_inner(&self, event: &NostrEvent) -> Result<ProcessResult> {
        // 1. Verify Nostr signature
        match verify_nostr_event(event) {
            Ok(true) => {}
            Ok(false) => {
                return Ok(ProcessResult::Skipped {
                    reason: "invalid signature".to_string(),
                });
            }
            Err(e) => {
                return Ok(ProcessResult::Skipped {
                    reason: format!("signature verification error: {e}"),
                });
            }
        }

        // 2. Check if user is linked and opted in
        let account = self
            .account_store
            .get_account_link(&event.pubkey)
            .await
            .context("failed to look up account link")?;

        let account = match account {
            Some(a) if a.opted_in => a,
            Some(_) => {
                return Ok(ProcessResult::Skipped {
                    reason: "user has not opted in".to_string(),
                });
            }
            None => {
                return Ok(ProcessResult::Skipped {
                    reason: "unknown pubkey — no account link".to_string(),
                });
            }
        };

        // 3. Handle deletion events (kind 5)
        if event.kind == 5 {
            return self.handle_deletion(event, &account).await;
        }

        // 4. Check idempotency
        if self
            .record_store
            .is_event_processed(&event.id)
            .await
            .context("failed to check idempotency")?
        {
            return Ok(ProcessResult::Skipped {
                reason: "event already processed".to_string(),
            });
        }

        // 5. For video events (kinds 34235, 34236)
        if event.kind == 34235 || event.kind == 34236 {
            return self.handle_video_event(event, &account).await;
        }

        Ok(ProcessResult::Skipped {
            reason: format!("unsupported event kind: {}", event.kind),
        })
    }

    async fn handle_video_event(
        &self,
        event: &NostrEvent,
        account: &AccountLink,
    ) -> Result<ProcessResult> {
        // Fetch blob from Blossom
        let video_url = get_video_url(event).context("no video URL found in event")?;

        let (blob_data, mime_type) = self
            .blob_fetcher
            .fetch_blob(&video_url)
            .await
            .context("failed to fetch blob")?;

        // Upload blob to PDS
        let blob_ref = self
            .blob_uploader
            .upload_blob(&blob_data, &mime_type)
            .await
            .context("failed to upload blob to PDS")?;

        // Translate event to ATProto post
        let post = translate_nip71_to_post(event, &blob_ref)
            .context("failed to translate event to ATProto post")?;

        let record_value =
            serde_json::to_value(&post).context("failed to serialize ATProto post")?;

        // Derive rkey and write record
        let rkey = derive_rkey(event);
        let collection = "app.bsky.feed.post";

        let at_uri = self
            .pds_publisher
            .put_record(&account.did, collection, &rkey, &record_value)
            .await
            .context("failed to write record to PDS")?;

        // Save mapping
        self.record_store
            .save_record_mapping(RecordMapping {
                nostr_event_id: event.id.clone(),
                at_uri: at_uri.clone(),
                did: account.did.clone(),
                collection: collection.to_string(),
                rkey: rkey.clone(),
                deleted: false,
            })
            .await
            .context("failed to save record mapping")?;

        Ok(ProcessResult::Published { at_uri, rkey })
    }

    async fn handle_deletion(
        &self,
        event: &NostrEvent,
        account: &AccountLink,
    ) -> Result<ProcessResult> {
        let target_id = match get_deleted_event_id(event) {
            Some(id) => id.to_string(),
            None => {
                return Ok(ProcessResult::Skipped {
                    reason: "deletion event has no 'e' tag".to_string(),
                });
            }
        };

        let mapping = match self
            .record_store
            .get_mapping_by_nostr_id(&target_id)
            .await
            .context("failed to look up record mapping")?
        {
            Some(m) => m,
            None => {
                return Ok(ProcessResult::Skipped {
                    reason: "no record mapping found for deleted event".to_string(),
                });
            }
        };

        if mapping.deleted {
            return Ok(ProcessResult::Skipped {
                reason: "record already deleted".to_string(),
            });
        }

        if mapping.deleted {
            return Ok(ProcessResult::Skipped {
                reason: "record already deleted".to_string(),
            });
        }

        if let Err(err) = validate_delete_request(event, &account.did, &mapping.did) {
            return Ok(ProcessResult::Skipped {
                reason: err.to_string(),
            });
        }

        self.pds_publisher
            .delete_record(&mapping.did, &mapping.collection, &mapping.rkey)
            .await
            .context("failed to delete record from PDS")?;

        self.record_store
            .mark_deleted(&target_id)
            .await
            .context("failed to mark record as deleted")?;

        Ok(ProcessResult::Deleted {
            at_uri: mapping.at_uri,
        })
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::rand::rngs::OsRng;
    use secp256k1::{Keypair, Secp256k1};
    use sha2::{Digest, Sha256};
    use std::sync::Mutex;

    // -----------------------------------------------------------------------
    // Test helpers: create signed Nostr events
    // -----------------------------------------------------------------------

    fn make_signed_event(
        kind: u64,
        content: &str,
        tags: Vec<Vec<String>>,
    ) -> NostrEvent {
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

    /// Build a signed video event with a URL tag.
    fn make_video_event(pubkey: &str) -> NostrEvent {
        // We need a properly signed event, so we create one then override pubkey
        // But that would break the signature. Instead, we create a full signed event
        // and return it along with its pubkey.
        // For the pipeline, the signature must be valid, so we use the generated pubkey.
        make_signed_event(
            34235,
            "",
            vec![
                vec!["title".into(), "Test Video".into()],
                vec!["url".into(), "https://blossom.example/video.mp4".into()],
                vec!["d".into(), "test-video".into()],
            ],
        )
    }

    fn make_deletion_event_for(target_id: &str) -> NostrEvent {
        make_signed_event(
            5,
            "",
            vec![vec!["e".into(), target_id.into()]],
        )
    }

    // -----------------------------------------------------------------------
    // Mock implementations
    // -----------------------------------------------------------------------

    struct MockAccountStore {
        links: Vec<AccountLink>,
    }

    #[async_trait]
    impl AccountStore for MockAccountStore {
        async fn get_account_link(&self, nostr_pubkey: &str) -> Result<Option<AccountLink>> {
            Ok(self.links.iter().find(|l| l.nostr_pubkey == nostr_pubkey).cloned())
        }
    }

    struct MockRecordStore {
        processed_ids: Vec<String>,
        mappings: Vec<RecordMapping>,
        saved: Mutex<Vec<RecordMapping>>,
        deleted: Mutex<Vec<String>>,
    }

    impl MockRecordStore {
        fn new() -> Self {
            Self {
                processed_ids: vec![],
                mappings: vec![],
                saved: Mutex::new(vec![]),
                deleted: Mutex::new(vec![]),
            }
        }

        fn with_processed(mut self, ids: Vec<String>) -> Self {
            self.processed_ids = ids;
            self
        }

        fn with_mappings(mut self, mappings: Vec<RecordMapping>) -> Self {
            self.mappings = mappings;
            self
        }
    }

    #[async_trait]
    impl RecordStore for MockRecordStore {
        async fn is_event_processed(&self, event_id: &str) -> Result<bool> {
            Ok(self.processed_ids.contains(&event_id.to_string()))
        }

        async fn save_record_mapping(&self, mapping: RecordMapping) -> Result<()> {
            self.saved.lock().unwrap().push(mapping);
            Ok(())
        }

        async fn get_mapping_by_nostr_id(&self, event_id: &str) -> Result<Option<RecordMapping>> {
            Ok(self
                .mappings
                .iter()
                .find(|m| m.nostr_event_id == event_id)
                .cloned())
        }

        async fn mark_deleted(&self, event_id: &str) -> Result<()> {
            self.deleted.lock().unwrap().push(event_id.to_string());
            Ok(())
        }
    }

    struct MockBlobFetcher;

    #[async_trait]
    impl BlobFetcher for MockBlobFetcher {
        async fn fetch_blob(&self, _url: &str) -> Result<(Vec<u8>, String)> {
            Ok((vec![0xDE, 0xAD, 0xBE, 0xEF], "video/mp4".to_string()))
        }
    }

    struct MockBlobUploader;

    #[async_trait]
    impl BlobUploader for MockBlobUploader {
        async fn upload_blob(&self, _data: &[u8], _mime_type: &str) -> Result<BlobRef> {
            Ok(BlobRef::new("bafkreiuploadedblob".to_string(), "video/mp4".to_string(), 4))
        }
    }

    struct MockPdsPublisher {
        published: Mutex<Vec<(String, String, String)>>, // (did, collection, rkey)
        deleted: Mutex<Vec<(String, String, String)>>,
    }

    impl MockPdsPublisher {
        fn new() -> Self {
            Self {
                published: Mutex::new(vec![]),
                deleted: Mutex::new(vec![]),
            }
        }
    }

    #[async_trait]
    impl PdsPublisher for MockPdsPublisher {
        async fn put_record(
            &self,
            did: &str,
            collection: &str,
            rkey: &str,
            _record: &serde_json::Value,
        ) -> Result<String> {
            self.published
                .lock()
                .unwrap()
                .push((did.to_string(), collection.to_string(), rkey.to_string()));
            Ok(format!("at://{}/{}/{}", did, collection, rkey))
        }

        async fn delete_record(
            &self,
            did: &str,
            collection: &str,
            rkey: &str,
        ) -> Result<()> {
            self.deleted
                .lock()
                .unwrap()
                .push((did.to_string(), collection.to_string(), rkey.to_string()));
            Ok(())
        }
    }

    // -----------------------------------------------------------------------
    // Helper to build a pipeline with the given account linked
    // -----------------------------------------------------------------------

    fn make_pipeline(
        account_store: MockAccountStore,
        record_store: MockRecordStore,
    ) -> BridgePipeline<MockAccountStore, MockRecordStore, MockBlobFetcher, MockBlobUploader, MockPdsPublisher>
    {
        BridgePipeline::new(
            account_store,
            record_store,
            MockBlobFetcher,
            MockBlobUploader,
            MockPdsPublisher::new(),
        )
    }

    fn account_for(pubkey: &str) -> AccountLink {
        AccountLink {
            nostr_pubkey: pubkey.to_string(),
            did: "did:plc:testuser".to_string(),
            opted_in: true,
        }
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn happy_path_video_event_published() {
        let event = make_video_event("ignored"); // pubkey comes from signing
        let accounts = MockAccountStore {
            links: vec![account_for(&event.pubkey)],
        };
        let records = MockRecordStore::new();
        let pipeline = make_pipeline(accounts, records);

        let result = pipeline.process_event(&event).await;

        match &result {
            ProcessResult::Published { at_uri, rkey } => {
                assert!(at_uri.contains("did:plc:testuser"));
                assert!(at_uri.contains("app.bsky.feed.post"));
                assert_eq!(rkey, "test-video");
            }
            other => panic!("expected Published, got {:?}", other),
        }

        // Verify record was saved
        let saved = pipeline.record_store.saved.lock().unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].nostr_event_id, event.id);
        assert_eq!(saved[0].rkey, "test-video");
    }

    #[tokio::test]
    async fn unlinked_user_skipped() {
        let event = make_video_event("unknown");
        let accounts = MockAccountStore { links: vec![] }; // no links
        let records = MockRecordStore::new();
        let pipeline = make_pipeline(accounts, records);

        let result = pipeline.process_event(&event).await;

        match &result {
            ProcessResult::Skipped { reason } => {
                assert!(reason.contains("unknown pubkey"), "got: {}", reason);
            }
            other => panic!("expected Skipped, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn duplicate_event_skipped() {
        let event = make_video_event("test");
        let accounts = MockAccountStore {
            links: vec![account_for(&event.pubkey)],
        };
        let records = MockRecordStore::new().with_processed(vec![event.id.clone()]);
        let pipeline = make_pipeline(accounts, records);

        let result = pipeline.process_event(&event).await;

        match &result {
            ProcessResult::Skipped { reason } => {
                assert!(reason.contains("already processed"), "got: {}", reason);
            }
            other => panic!("expected Skipped, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn invalid_signature_skipped() {
        let mut event = make_video_event("test");
        // Corrupt the signature
        let mut sig_bytes = hex::decode(&event.sig).unwrap();
        sig_bytes[0] ^= 0xff;
        event.sig = hex::encode(&sig_bytes);

        let accounts = MockAccountStore {
            links: vec![account_for(&event.pubkey)],
        };
        let records = MockRecordStore::new();
        let pipeline = make_pipeline(accounts, records);

        let result = pipeline.process_event(&event).await;

        match &result {
            ProcessResult::Skipped { reason } => {
                assert!(
                    reason.contains("invalid signature") || reason.contains("signature verification error"),
                    "got: {}",
                    reason
                );
            }
            other => panic!("expected Skipped, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn deletion_event_deletes_record() {
        // First create a video event to get its ID
        let video_event = make_video_event("test");
        let video_id = video_event.id.clone();
        // Now make a deletion event referencing that video
        let del_event = make_deletion_event_for(&video_id);

        let accounts = MockAccountStore {
            links: vec![account_for(&del_event.pubkey)],
        };
        let records = MockRecordStore::new().with_mappings(vec![RecordMapping {
            nostr_event_id: video_id.clone(),
            at_uri: format!(
                "at://did:plc:testuser/app.bsky.feed.post/{}",
                video_id
            ),
            did: "did:plc:testuser".to_string(),
            collection: "app.bsky.feed.post".to_string(),
            rkey: video_id.clone(),
            deleted: false,
        }]);
        let pipeline = make_pipeline(accounts, records);

        let result = pipeline.process_event(&del_event).await;

        match &result {
            ProcessResult::Deleted { at_uri } => {
                assert!(at_uri.contains("did:plc:testuser"));
            }
            other => panic!("expected Deleted, got {:?}", other),
        }

        // Verify deletion was recorded
        let deleted = pipeline.record_store.deleted.lock().unwrap();
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0], video_id);
    }

    #[tokio::test]
    async fn deletion_owner_mismatch_skipped() {
        let del_event = make_deletion_event_for("event-owned-by-someone-else");

        let accounts = MockAccountStore {
            links: vec![AccountLink {
                nostr_pubkey: del_event.pubkey.clone(),
                did: "did:plc:deleter".to_string(),
                opted_in: true,
            }],
        };
        let records = MockRecordStore::new().with_mappings(vec![RecordMapping {
            nostr_event_id: "event-owned-by-someone-else".to_string(),
            at_uri: "at://did:plc:owner/app.bsky.feed.post/rkey".to_string(),
            did: "did:plc:owner".to_string(),
            collection: "app.bsky.feed.post".to_string(),
            rkey: "rkey".to_string(),
            deleted: false,
        }]);
        let pipeline = make_pipeline(accounts, records);

        let result = pipeline.process_event(&del_event).await;

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
    async fn deletion_with_no_mapping_skipped() {
        let del_event = make_deletion_event_for("nonexistent-event");

        let accounts = MockAccountStore {
            links: vec![account_for(&del_event.pubkey)],
        };
        let records = MockRecordStore::new(); // no mappings
        let pipeline = make_pipeline(accounts, records);

        let result = pipeline.process_event(&del_event).await;

        match &result {
            ProcessResult::Skipped { reason } => {
                assert!(reason.contains("no record mapping"), "got: {}", reason);
            }
            other => panic!("expected Skipped, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn user_not_opted_in_skipped() {
        let event = make_video_event("test");
        let mut link = account_for(&event.pubkey);
        link.opted_in = false;
        let accounts = MockAccountStore { links: vec![link] };
        let records = MockRecordStore::new();
        let pipeline = make_pipeline(accounts, records);

        let result = pipeline.process_event(&event).await;

        match &result {
            ProcessResult::Skipped { reason } => {
                assert!(reason.contains("not opted in"), "got: {}", reason);
            }
            other => panic!("expected Skipped, got {:?}", other),
        }
    }
}
