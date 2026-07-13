//! Bridge Orchestrator — the main pipeline that wires together all modules.
//!
//! Flow: nostr_consumer → signature verify → translator → blob upload → publisher
//!
//! Uses trait-based abstractions for all external dependencies so the
//! orchestration logic is pure and fully testable with mocks.

use anyhow::{Context, Result};
use async_trait::async_trait;
use divine_bridge_types::{BlobRef, NostrEvent, RecordStatus};
use divine_video_worker::normalize::prepare_publishable_video;
use divine_video_worker::profile_image::{prepare_profile_image, ProfileImageKind};
use sha2::{Digest, Sha256};
use std::time::Duration;

use crate::deletion::validate_delete_request;
use crate::profile_sync::{
    build_profile_record, parse_kind0_profile, profile_assets, PROFILE_COLLECTION, PROFILE_RKEY,
};
use crate::signature::verify_nostr_event;
use crate::translator::{get_text_tracks, translate_nip71_to_post, VideoCaption};

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

/// Persisted lineage for a bridged media asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetManifestRecord {
    pub source_sha256: String,
    pub blossom_url: Option<String>,
    pub at_blob_cid: String,
    pub mime: String,
    pub bytes: u64,
    pub is_derivative: bool,
}

/// Metadata returned from a verified blob fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedBlob {
    pub data: Vec<u8>,
    pub mime_type: String,
    pub source_sha256: String,
}

/// Metadata returned from a PDS write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedRecord {
    pub at_uri: String,
    pub rkey: String,
    pub cid: Option<String>,
}

/// Result of processing a single Nostr event through the pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessResult {
    Published { at_uri: String, rkey: String },
    ProfileSynced { at_uri: String, rkey: String },
    Deleted { at_uri: String },
    Skipped { reason: String },
    Error { message: String },
}

/// Queue-ready payload for a publish or tombstone job.
#[derive(Debug, Clone, PartialEq)]
pub struct PublishJobEnvelope {
    pub nostr_event_id: String,
    pub nostr_pubkey: String,
    pub event_created_at: i64,
    pub event_payload: serde_json::Value,
    /// Durable AT record key reserved before any remote side effect.
    pub reserved_rkey: Option<String>,
    /// Exact canonical record selected before the first PDS create attempt.
    pub prepared_record: Option<serde_json::Value>,
}

/// Prepare-phase decision for a relay event.
#[derive(Debug, Clone, PartialEq)]
pub enum QueueDecision {
    Enqueue(PublishJobEnvelope),
    Cancel {
        target_nostr_event_id: String,
        tombstone_job: PublishJobEnvelope,
    },
    Skip {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DedupeMode {
    QueueAware,
    PublishedOnly,
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
    async fn save_asset_manifest(&self, _entry: AssetManifestRecord) -> Result<()> {
        Ok(())
    }
    async fn reserve_prepared_record(
        &self,
        _event_id: &str,
        candidate: serde_json::Value,
    ) -> Result<serde_json::Value> {
        Ok(candidate)
    }
    async fn update_record_mapping_status(
        &self,
        _event_id: &str,
        _cid: Option<&str>,
        _status: RecordStatus,
    ) -> Result<()> {
        Ok(())
    }
}

/// Fetch a blob from a Blossom server (or other source).
#[async_trait]
pub trait BlobFetcher: Send + Sync {
    /// Returns (bytes, mime_type).
    async fn fetch_blob(&self, url: &str) -> Result<(Vec<u8>, String)>;
    async fn fetch_blob_verified(
        &self,
        url: &str,
        expected_sha256: Option<&str>,
    ) -> Result<FetchedBlob> {
        let (data, mime_type) = self.fetch_blob(url).await?;
        let actual_sha256 = hex::encode(Sha256::digest(&data));
        if let Some(expected_sha256) = expected_sha256 {
            anyhow::ensure!(
                actual_sha256 == expected_sha256,
                "SHA-256 mismatch: expected {}, got {}",
                expected_sha256,
                actual_sha256
            );
        }

        Ok(FetchedBlob {
            data,
            mime_type,
            source_sha256: actual_sha256,
        })
    }
}

/// Upload a blob to a PDS (or video transcoding service).
#[async_trait]
pub trait BlobUploader: Send + Sync {
    async fn upload_blob(&self, data: &[u8], mime_type: &str) -> Result<BlobRef>;

    /// Upload a blob on behalf of a specific user DID.
    ///
    /// Implementations that need the user DID (e.g. video service) override
    /// this; the default delegates to [`upload_blob`](Self::upload_blob).
    async fn upload_blob_for_user(
        &self,
        data: &[u8],
        mime_type: &str,
        _user_did: &str,
    ) -> Result<BlobRef> {
        self.upload_blob(data, mime_type).await
    }
}

#[async_trait]
impl BlobUploader for Box<dyn BlobUploader> {
    async fn upload_blob(&self, data: &[u8], mime_type: &str) -> Result<BlobRef> {
        (**self).upload_blob(data, mime_type).await
    }

    async fn upload_blob_for_user(
        &self,
        data: &[u8],
        mime_type: &str,
        user_did: &str,
    ) -> Result<BlobRef> {
        (**self)
            .upload_blob_for_user(data, mime_type, user_did)
            .await
    }
}

/// Publish / delete records on a PDS.
#[async_trait]
pub trait PdsPublisher: Send + Sync {
    async fn create_record(
        &self,
        did: &str,
        collection: &str,
        record: &serde_json::Value,
    ) -> Result<String>;
    async fn create_record_with_meta(
        &self,
        did: &str,
        collection: &str,
        record: &serde_json::Value,
    ) -> Result<PublishedRecord> {
        let at_uri = self.create_record(did, collection, record).await?;
        let rkey = parse_rkey_from_at_uri(&at_uri)?;
        Ok(PublishedRecord {
            at_uri,
            rkey,
            cid: None,
        })
    }

    async fn create_record_at_rkey_with_meta(
        &self,
        _did: &str,
        _collection: &str,
        _rkey: &str,
        _record: &serde_json::Value,
    ) -> Result<PublishedRecord> {
        anyhow::bail!("publisher does not support create-only writes at a reserved rkey")
    }

    async fn put_record(
        &self,
        did: &str,
        collection: &str,
        rkey: &str,
        record: &serde_json::Value,
    ) -> Result<String>; // returns at_uri
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
            cid: None,
        })
    }

    async fn delete_record(&self, did: &str, collection: &str, rkey: &str) -> Result<()>;
}

/// Reqwest-backed blob fetcher with bounded network timeouts.
#[derive(Debug, Clone)]
pub struct HttpBlobFetcher {
    client: reqwest::Client,
}

impl HttpBlobFetcher {
    pub fn new(timeout: Duration) -> Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(timeout)
            .build()
            .context("failed to build blob fetch client")?;
        Ok(Self { client })
    }
}

#[async_trait]
impl BlobFetcher for HttpBlobFetcher {
    async fn fetch_blob(&self, url: &str) -> Result<(Vec<u8>, String)> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .context("failed to fetch blob")?;

        let status = response.status();
        anyhow::ensure!(
            status.is_success(),
            "blob fetch failed with HTTP {}",
            status
        );

        let mime_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();

        let data = response
            .bytes()
            .await
            .context("failed to read blob response body")?
            .to_vec();

        Ok((data, mime_type))
    }
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

/// Extract a source SHA-256 from `x` or `imeta x` tags.
fn get_source_sha256(event: &NostrEvent) -> Option<String> {
    for tag in &event.tags {
        if tag.len() >= 2 && tag[0] == "x" {
            return Some(tag[1].clone());
        }
    }
    for tag in &event.tags {
        if tag.first().map(|s| s.as_str()) == Some("imeta") {
            for entry in &tag[1..] {
                if let Some(val) = entry.strip_prefix("x ") {
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

fn parse_rkey_from_at_uri(at_uri: &str) -> Result<String> {
    at_uri
        .rsplit('/')
        .next()
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .context("published AT-URI is missing an rkey segment")
}

fn build_publish_job_envelope(
    event: &NostrEvent,
    nostr_event_id: String,
) -> Result<PublishJobEnvelope> {
    Ok(PublishJobEnvelope {
        nostr_event_id,
        nostr_pubkey: event.pubkey.clone(),
        event_created_at: event.created_at,
        event_payload: serde_json::to_value(event).context("failed to serialize event payload")?,
        reserved_rkey: None,
        prepared_record: None,
    })
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

    /// Classify a relay event into a queue decision without performing publish side effects.
    pub async fn prepare_publish_job(&self, event: &NostrEvent) -> Result<QueueDecision> {
        // 1. Verify Nostr signature
        match verify_nostr_event(event) {
            Ok(true) => {}
            Ok(false) => {
                return Ok(QueueDecision::Skip {
                    reason: "invalid signature".to_string(),
                });
            }
            Err(e) => {
                return Ok(QueueDecision::Skip {
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

        match account {
            Some(a) if a.opted_in => a,
            Some(_) => {
                return Ok(QueueDecision::Skip {
                    reason: "user has not opted in".to_string(),
                });
            }
            None => {
                return Ok(QueueDecision::Skip {
                    reason: "unknown pubkey — no account link".to_string(),
                });
            }
        };

        // 3. Handle deletion events as queue cancellation intents.
        if event.kind == 5 {
            let target_id = match get_deleted_event_id(event) {
                Some(id) => id.to_string(),
                None => {
                    return Ok(QueueDecision::Skip {
                        reason: "deletion event has no 'e' tag".to_string(),
                    });
                }
            };
            return Ok(QueueDecision::Cancel {
                target_nostr_event_id: target_id.clone(),
                tombstone_job: build_publish_job_envelope(event, target_id)?,
            });
        }

        // 4. Check idempotency before enqueueing.
        if self
            .record_store
            .is_event_processed(&event.id)
            .await
            .context("failed to check idempotency")?
        {
            return Ok(QueueDecision::Skip {
                reason: "event already processed".to_string(),
            });
        }

        // 5. Enqueue supported kinds.
        if event.kind == 0 || event.kind == 34235 || event.kind == 34236 {
            return Ok(QueueDecision::Enqueue(build_publish_job_envelope(
                event,
                event.id.clone(),
            )?));
        }

        Ok(QueueDecision::Skip {
            reason: format!("unsupported event kind: {}", event.kind),
        })
    }

    /// Execute a queued publish job using only the persisted event payload.
    pub async fn execute_publish_job(&self, job: &PublishJobEnvelope) -> Result<ProcessResult> {
        let event: NostrEvent = serde_json::from_value(job.event_payload.clone())
            .context("failed to deserialize queued event payload")?;
        anyhow::ensure!(
            event.kind != 34235 && event.kind != 34236 || job.reserved_rkey.is_some(),
            "queued video publication requires a durable reserved rkey"
        );
        self.process_event_inner(
            &event,
            DedupeMode::PublishedOnly,
            job.reserved_rkey.as_deref(),
            job.prepared_record.as_ref(),
        )
        .await
    }

    /// Process a single Nostr event through the full bridge pipeline.
    pub async fn process_event(&self, event: &NostrEvent) -> ProcessResult {
        match self
            .process_event_inner(event, DedupeMode::QueueAware, None, None)
            .await
        {
            Ok(result) => result,
            Err(e) => ProcessResult::Error {
                message: format!("{e:#}"),
            },
        }
    }

    async fn process_event_inner(
        &self,
        event: &NostrEvent,
        dedupe_mode: DedupeMode,
        reserved_rkey: Option<&str>,
        prepared_record: Option<&serde_json::Value>,
    ) -> Result<ProcessResult> {
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
        let already_processed = match dedupe_mode {
            DedupeMode::QueueAware => self
                .record_store
                .is_event_processed(&event.id)
                .await
                .context("failed to check idempotency")?,
            DedupeMode::PublishedOnly => self
                .record_store
                .get_mapping_by_nostr_id(&event.id)
                .await
                .context("failed to look up record mapping")?
                .is_some(),
        };
        if already_processed {
            return Ok(ProcessResult::Skipped {
                reason: "event already processed".to_string(),
            });
        }

        if event.kind == 0 {
            return self.handle_profile_event(event, &account).await;
        }

        // 5. For video events (kinds 34235, 34236)
        if event.kind == 34235 || event.kind == 34236 {
            return self
                .handle_video_event(event, &account, reserved_rkey, prepared_record)
                .await;
        }

        Ok(ProcessResult::Skipped {
            reason: format!("unsupported event kind: {}", event.kind),
        })
    }

    /// Fetch, validate, and upload the event's NIP-71 `text-track` WebVTT
    /// files, returning caption entries for the video embed. Best-effort:
    /// every failure is logged and skipped so captions can never block a
    /// video publish.
    async fn resolve_video_captions(&self, event: &NostrEvent, did: &str) -> Vec<VideoCaption> {
        // app.bsky.embed.video lexicon limits.
        const MAX_VIDEO_CAPTIONS: usize = 20;
        const MAX_CAPTION_VTT_BYTES: usize = 20_000;

        let mut captions = Vec::new();
        for track in get_text_tracks(event) {
            if captions.len() >= MAX_VIDEO_CAPTIONS {
                tracing::warn!(
                    nostr_event_id = %event.id,
                    "more than {MAX_VIDEO_CAPTIONS} text-tracks; ignoring the rest"
                );
                break;
            }

            // Divine's text-track URLs are content-addressed: verify when the
            // last path segment looks like a sha256.
            let expected_sha256 = track
                .url
                .rsplit('/')
                .next()
                .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
                .map(str::to_ascii_lowercase);

            let fetched = match self
                .blob_fetcher
                .fetch_blob_verified(&track.url, expected_sha256.as_deref())
                .await
            {
                Ok(fetched) => fetched,
                Err(error) => {
                    tracing::warn!(
                        nostr_event_id = %event.id,
                        url = %track.url,
                        error = %format!("{error:#}"),
                        "failed to fetch text-track; skipping caption"
                    );
                    continue;
                }
            };

            let body = fetched
                .data
                .strip_prefix(&[0xEF, 0xBB, 0xBF][..])
                .unwrap_or(&fetched.data);
            if !body.starts_with(b"WEBVTT") {
                tracing::warn!(
                    nostr_event_id = %event.id,
                    url = %track.url,
                    "text-track is not WebVTT; skipping caption"
                );
                continue;
            }
            if fetched.data.len() > MAX_CAPTION_VTT_BYTES {
                tracing::warn!(
                    nostr_event_id = %event.id,
                    url = %track.url,
                    size = fetched.data.len(),
                    "text-track exceeds the {MAX_CAPTION_VTT_BYTES}-byte caption limit; skipping"
                );
                continue;
            }

            match self
                .blob_uploader
                .upload_blob_for_user(&fetched.data, "text/vtt", did)
                .await
            {
                Ok(file) => captions.push(VideoCaption {
                    type_: "app.bsky.embed.video#caption".to_string(),
                    lang: track.lang,
                    file,
                }),
                Err(error) => {
                    tracing::warn!(
                        nostr_event_id = %event.id,
                        url = %track.url,
                        error = %format!("{error:#}"),
                        "failed to upload caption blob; skipping caption"
                    );
                }
            }
        }
        captions
    }

    async fn handle_video_event(
        &self,
        event: &NostrEvent,
        account: &AccountLink,
        reserved_rkey: Option<&str>,
        prepared_record: Option<&serde_json::Value>,
    ) -> Result<ProcessResult> {
        let video_url = get_video_url(event).context("no video URL found in event")?;
        let expected_sha256 = get_source_sha256(event).context("no source hash found in event")?;
        let record_value = match prepared_record {
            Some(record) => record.clone(),
            None => {
                let fetched = self
                    .blob_fetcher
                    .fetch_blob_verified(&video_url, Some(&expected_sha256))
                    .await
                    .context("failed to fetch blob")?;

                let prepared_video = prepare_publishable_video(&fetched.data, &fetched.mime_type)
                    .context("failed to prepare publishable video")?;

                let blob_ref = self
                    .blob_uploader
                    .upload_blob_for_user(
                        &prepared_video.data,
                        &prepared_video.mime_type,
                        &account.did,
                    )
                    .await
                    .context("failed to upload blob to PDS")?;

                let mut post = translate_nip71_to_post(event, &blob_ref)
                    .context("failed to translate event to ATProto post")?;
                if let Some(embed) = post.embed.as_mut() {
                    embed.captions = self.resolve_video_captions(event, &account.did).await;
                }

                let candidate =
                    serde_json::to_value(&post).context("failed to serialize ATProto post")?;
                self.record_store
                    .reserve_prepared_record(&event.id, candidate)
                    .await
                    .context("failed to persist prepared AT record")?
            }
        };

        // Validate the durable intent before any create. This also provides the
        // lineage values on recovery without repeating media work.
        let blob_ref: BlobRef = serde_json::from_value(
            record_value
                .pointer("/embed/video")
                .cloned()
                .context("prepared AT record is missing embed.video")?,
        )
        .context("prepared AT record contains an invalid video blob")?;

        let collection = "app.bsky.feed.post";

        let published = match reserved_rkey {
            Some(rkey) => {
                self.pds_publisher
                    .create_record_at_rkey_with_meta(&account.did, collection, rkey, &record_value)
                    .await
            }
            None => {
                self.pds_publisher
                    .create_record_with_meta(&account.did, collection, &record_value)
                    .await
            }
        }
        .context("failed to write record to PDS")?;

        // Save mapping
        self.record_store
            .save_record_mapping(RecordMapping {
                nostr_event_id: event.id.clone(),
                at_uri: published.at_uri.clone(),
                did: account.did.clone(),
                collection: collection.to_string(),
                rkey: published.rkey.clone(),
                deleted: false,
            })
            .await
            .context("failed to save record mapping")?;

        self.record_store
            .save_asset_manifest(AssetManifestRecord {
                source_sha256: expected_sha256,
                blossom_url: Some(video_url),
                at_blob_cid: blob_ref.cid().to_string(),
                mime: blob_ref.mime_type,
                bytes: blob_ref.size,
                is_derivative: false,
            })
            .await
            .context("failed to save asset manifest")?;

        self.record_store
            .update_record_mapping_status(
                &event.id,
                published.cid.as_deref(),
                RecordStatus::Published,
            )
            .await
            .context("failed to update record status")?;

        Ok(ProcessResult::Published {
            at_uri: published.at_uri,
            rkey: published.rkey,
        })
    }

    async fn handle_profile_event(
        &self,
        event: &NostrEvent,
        account: &AccountLink,
    ) -> Result<ProcessResult> {
        let parsed = parse_kind0_profile(event).context("failed to parse kind 0 profile")?;
        let assets = profile_assets(&parsed);

        let avatar = match assets.avatar_url {
            Some(url) => {
                let fetched = self
                    .blob_fetcher
                    .fetch_blob_verified(&url, None)
                    .await
                    .context("failed to fetch avatar")?;
                let prepared = prepare_profile_image(
                    &fetched.data,
                    &fetched.mime_type,
                    ProfileImageKind::Avatar,
                )
                .context("failed to prepare avatar")?;
                Some(
                    self.blob_uploader
                        .upload_blob_for_user(&prepared.data, &prepared.mime_type, &account.did)
                        .await
                        .context("failed to upload avatar")?,
                )
            }
            None => None,
        };

        let banner = match assets.banner_url {
            Some(url) => {
                let fetched = self
                    .blob_fetcher
                    .fetch_blob_verified(&url, None)
                    .await
                    .context("failed to fetch banner")?;
                let prepared = prepare_profile_image(
                    &fetched.data,
                    &fetched.mime_type,
                    ProfileImageKind::Banner,
                )
                .context("failed to prepare banner")?;
                Some(
                    self.blob_uploader
                        .upload_blob_for_user(&prepared.data, &prepared.mime_type, &account.did)
                        .await
                        .context("failed to upload banner")?,
                )
            }
            None => None,
        };

        let record = build_profile_record(&parsed, avatar, banner);
        let published = self
            .pds_publisher
            .put_record_with_meta(&account.did, PROFILE_COLLECTION, PROFILE_RKEY, &record)
            .await
            .context("failed to write profile to PDS")?;

        self.record_store
            .save_record_mapping(RecordMapping {
                nostr_event_id: event.id.clone(),
                at_uri: published.at_uri.clone(),
                did: account.did.clone(),
                collection: PROFILE_COLLECTION.to_string(),
                rkey: published.rkey.clone(),
                deleted: false,
            })
            .await
            .context("failed to save profile mapping")?;

        self.record_store
            .update_record_mapping_status(
                &event.id,
                published.cid.as_deref(),
                RecordStatus::Published,
            )
            .await
            .context("failed to update profile status")?;

        Ok(ProcessResult::ProfileSynced {
            at_uri: published.at_uri,
            rkey: published.rkey,
        })
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
    use std::sync::{Arc, Mutex};

    // -----------------------------------------------------------------------
    // Test helpers: create signed Nostr events
    // -----------------------------------------------------------------------

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

    /// Build a signed video event with a URL tag.
    fn make_video_event(_pubkey: &str) -> NostrEvent {
        let payload = [0xDE, 0xAD, 0xBE, 0xEF];
        let source_sha256 = hex::encode(Sha256::digest(payload));
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
                vec!["x".into(), source_sha256],
                vec!["d".into(), "test-video".into()],
            ],
        )
    }

    fn make_deletion_event_for(target_id: &str) -> NostrEvent {
        make_signed_event(5, "", vec![vec!["e".into(), target_id.into()]])
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
            Ok(self
                .links
                .iter()
                .find(|l| l.nostr_pubkey == nostr_pubkey)
                .cloned())
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
            Ok(BlobRef::new(
                "bafkreiuploadedblob".to_string(),
                "video/mp4".to_string(),
                4,
            ))
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
        async fn create_record(
            &self,
            did: &str,
            collection: &str,
            _record: &serde_json::Value,
        ) -> Result<String> {
            let rkey = "3mockvideorkey";
            self.published.lock().unwrap().push((
                did.to_string(),
                collection.to_string(),
                rkey.to_string(),
            ));
            Ok(format!("at://{}/{}/{}", did, collection, rkey))
        }

        async fn put_record(
            &self,
            did: &str,
            collection: &str,
            rkey: &str,
            _record: &serde_json::Value,
        ) -> Result<String> {
            self.published.lock().unwrap().push((
                did.to_string(),
                collection.to_string(),
                rkey.to_string(),
            ));
            Ok(format!("at://{}/{}/{}", did, collection, rkey))
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
                cid: Some("bafytestrecord".to_string()),
            })
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

    // -----------------------------------------------------------------------
    // Helper to build a pipeline with the given account linked
    // -----------------------------------------------------------------------

    fn make_pipeline(
        account_store: MockAccountStore,
        record_store: MockRecordStore,
    ) -> BridgePipeline<
        MockAccountStore,
        MockRecordStore,
        MockBlobFetcher,
        MockBlobUploader,
        MockPdsPublisher,
    > {
        BridgePipeline::new(
            account_store,
            record_store,
            MockBlobFetcher,
            MockBlobUploader,
            MockPdsPublisher::new(),
        )
    }

    /// Serves the video payload for video URLs and a configurable body (or an
    /// error) for `.vtt` URLs.
    struct CaptionScenarioFetcher {
        vtt: Option<Vec<u8>>,
    }

    #[async_trait]
    impl BlobFetcher for CaptionScenarioFetcher {
        async fn fetch_blob(&self, url: &str) -> Result<(Vec<u8>, String)> {
            if url.ends_with(".vtt") {
                match &self.vtt {
                    Some(body) => Ok((body.clone(), "text/vtt".to_string())),
                    None => anyhow::bail!("vtt fetch failed"),
                }
            } else {
                Ok((vec![0xDE, 0xAD, 0xBE, 0xEF], "video/mp4".to_string()))
            }
        }
    }

    /// Records the (mime_type, did) of every per-account blob upload.
    struct AccountUploadRecordingUploader {
        calls: Arc<Mutex<Vec<(String, String)>>>,
    }

    #[async_trait]
    impl BlobUploader for AccountUploadRecordingUploader {
        async fn upload_blob(&self, _data: &[u8], _mime_type: &str) -> Result<BlobRef> {
            anyhow::bail!("shared-token upload_blob must not be used for account blobs")
        }

        async fn upload_blob_for_user(
            &self,
            _data: &[u8],
            mime_type: &str,
            user_did: &str,
        ) -> Result<BlobRef> {
            self.calls
                .lock()
                .unwrap()
                .push((mime_type.to_string(), user_did.to_string()));
            Ok(BlobRef::new(
                "bafkreiuploadedblob".to_string(),
                mime_type.to_string(),
                4,
            ))
        }
    }

    /// Captures every record JSON handed to the publisher.
    struct RecordCapturingPublisher {
        records: Mutex<Vec<serde_json::Value>>,
    }

    impl RecordCapturingPublisher {
        fn new() -> Self {
            Self {
                records: Mutex::new(vec![]),
            }
        }
    }

    #[async_trait]
    impl PdsPublisher for RecordCapturingPublisher {
        async fn create_record(
            &self,
            did: &str,
            collection: &str,
            record: &serde_json::Value,
        ) -> Result<String> {
            self.records.lock().unwrap().push(record.clone());
            Ok(format!("at://{}/{}/3capturedrkey", did, collection))
        }

        async fn put_record(
            &self,
            did: &str,
            collection: &str,
            rkey: &str,
            record: &serde_json::Value,
        ) -> Result<String> {
            self.records.lock().unwrap().push(record.clone());
            Ok(format!("at://{}/{}/{}", did, collection, rkey))
        }

        async fn delete_record(&self, _did: &str, _collection: &str, _rkey: &str) -> Result<()> {
            Ok(())
        }
    }

    fn make_captioned_video_event(vtt_url: &str) -> NostrEvent {
        let payload = [0xDE, 0xAD, 0xBE, 0xEF];
        let source_sha256 = hex::encode(Sha256::digest(payload));
        make_signed_event(
            34235,
            "",
            vec![
                vec!["title".into(), "Captioned Video".into()],
                vec!["url".into(), "https://blossom.example/video.mp4".into()],
                vec!["x".into(), source_sha256],
                vec!["d".into(), "captioned-video".into()],
                vec!["text-track".into(), vtt_url.into(), "wss://relay".into()],
                vec!["L".into(), "ISO-639-1".into()],
                vec!["l".into(), "pt".into(), "ISO-639-1".into()],
            ],
        )
    }

    async fn run_caption_scenario(vtt: Option<Vec<u8>>) -> (ProcessResult, Vec<serde_json::Value>) {
        let event = make_captioned_video_event("https://media.example/captions.vtt");
        let accounts = MockAccountStore {
            links: vec![account_for(&event.pubkey)],
        };
        let pipeline = BridgePipeline::new(
            accounts,
            MockRecordStore::new(),
            CaptionScenarioFetcher { vtt },
            MockBlobUploader,
            RecordCapturingPublisher::new(),
        );
        let result = pipeline.process_event(&event).await;
        let records = pipeline.pds_publisher.records.lock().unwrap().clone();
        (result, records)
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
    async fn video_with_text_track_publishes_captions() {
        let vtt = b"WEBVTT\n\n00:00.000 --> 00:02.000\nhello".to_vec();
        let (result, records) = run_caption_scenario(Some(vtt)).await;

        assert!(matches!(result, ProcessResult::Published { .. }));
        let captions = &records[0]["embed"]["captions"];
        assert_eq!(captions.as_array().unwrap().len(), 1);
        assert_eq!(captions[0]["$type"], "app.bsky.embed.video#caption");
        assert_eq!(captions[0]["lang"], "pt");
        assert_eq!(captions[0]["file"]["ref"]["$link"], "bafkreiuploadedblob");
    }

    #[tokio::test]
    async fn caption_blobs_upload_as_the_account_not_the_shared_token() {
        // rsky authorizes repo writes per-DID: uploading a caption with the
        // shared admin token fails with `BadJwt` (it isn't a JWT at all).
        let event = make_captioned_video_event("https://media.example/captions.vtt");
        let calls = Arc::new(Mutex::new(vec![]));
        let pipeline = BridgePipeline::new(
            MockAccountStore {
                links: vec![account_for(&event.pubkey)],
            },
            MockRecordStore::new(),
            CaptionScenarioFetcher {
                vtt: Some(b"WEBVTT\n\n00:00.000 --> 00:02.000\nhi".to_vec()),
            },
            AccountUploadRecordingUploader {
                calls: calls.clone(),
            },
            RecordCapturingPublisher::new(),
        );

        let result = pipeline.process_event(&event).await;
        assert!(matches!(result, ProcessResult::Published { .. }));

        let calls = calls.lock().unwrap();
        assert!(
            calls
                .iter()
                .any(|(mime, did)| mime == "text/vtt" && did == "did:plc:testuser"),
            "caption blob must upload as the account, got {calls:?}"
        );
    }

    #[tokio::test]
    async fn caption_fetch_failure_still_publishes_post() {
        let (result, records) = run_caption_scenario(None).await;

        assert!(matches!(result, ProcessResult::Published { .. }));
        // Empty captions are skipped during serialization entirely.
        assert!(records[0]["embed"].get("captions").is_none());
    }

    #[tokio::test]
    async fn non_vtt_text_track_is_skipped_but_post_publishes() {
        let (result, records) = run_caption_scenario(Some(b"<html>not vtt</html>".to_vec())).await;

        assert!(matches!(result, ProcessResult::Published { .. }));
        assert!(records[0]["embed"].get("captions").is_none());
    }

    #[tokio::test]
    async fn oversized_vtt_is_skipped_but_post_publishes() {
        let mut vtt = b"WEBVTT\n".to_vec();
        vtt.resize(20_001, b' ');
        let (result, records) = run_caption_scenario(Some(vtt)).await;

        assert!(matches!(result, ProcessResult::Published { .. }));
        assert!(records[0]["embed"].get("captions").is_none());
    }

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
                assert_eq!(rkey, "3mockvideorkey");
            }
            other => panic!("expected Published, got {:?}", other),
        }

        // Verify record was saved
        let saved = pipeline.record_store.saved.lock().unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].nostr_event_id, event.id);
        assert_eq!(saved[0].rkey, "3mockvideorkey");
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
                    reason.contains("invalid signature")
                        || reason.contains("signature verification error"),
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
            at_uri: format!("at://did:plc:testuser/app.bsky.feed.post/{}", video_id),
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
