//! Shared types for the Divine Bridge between Nostr and AT Protocol.
//!
//! This crate holds type definitions used across bridge crates: Nostr event types,
//! ATProto record types, blob references, publish job states, etc.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Nostr types
// ---------------------------------------------------------------------------

/// A minimal representation of a Nostr event relevant to the bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NostrEvent {
    pub id: String,
    pub pubkey: String,
    pub created_at: i64,
    pub kind: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub sig: String,
}

/// NIP-71 video event metadata extracted from tags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoMeta {
    pub title: Option<String>,
    pub url: String,
    pub thumb: Option<String>,
    pub summary: Option<String>,
    pub duration: Option<u64>,
    pub mime: Option<String>,
    pub sha256: Option<String>,
}

// ---------------------------------------------------------------------------
// ATProto types
// ---------------------------------------------------------------------------

/// Reference to a blob stored in a PDS, serialized to match the ATProto blob schema:
/// `{"$type":"blob","ref":{"$link":"bafkrei..."},"mimeType":"video/mp4","size":1024000}`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobRef {
    #[serde(rename = "$type", default = "blob_type_default")]
    pub type_: String,
    #[serde(rename = "ref")]
    pub ref_link: CidLink,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub size: u64,
}

fn blob_type_default() -> String {
    "blob".to_string()
}

impl BlobRef {
    pub fn new(cid: String, mime_type: String, size: u64) -> Self {
        Self {
            type_: "blob".to_string(),
            ref_link: CidLink { link: cid },
            mime_type,
            size,
        }
    }

    /// Get the CID string.
    pub fn cid(&self) -> &str {
        &self.ref_link.link
    }
}

/// The `{"$link": "..."}` wrapper used in ATProto blob references.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CidLink {
    #[serde(rename = "$link")]
    pub link: String,
}

/// A record to be written to a PDS repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdsRecord {
    pub did: String,
    pub collection: String,
    pub rkey: String,
    pub record: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Bridge status / job types
// ---------------------------------------------------------------------------

/// States a publish job can be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishState {
    Pending,
    InProgress,
    Published,
    Failed,
    Skipped,
}

impl PublishState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Published => "published",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

impl std::fmt::Display for PublishState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// States a record mapping can be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordStatus {
    Published,
    Deleted,
    TakenDown,
}

impl RecordStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::Deleted => "deleted",
            Self::TakenDown => "taken_down",
        }
    }
}

impl std::fmt::Display for RecordStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Moderation action types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModerationAction {
    Takedown,
    Flag,
    Label,
    Restore,
}

impl ModerationAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Takedown => "takedown",
            Self::Flag => "flag",
            Self::Label => "label",
            Self::Restore => "restore",
        }
    }
}

/// Origin of a moderation action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModerationOrigin {
    Nostr,
    Atproto,
    Manual,
}

/// Checkpoint for relay ingestion progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestCheckpoint {
    pub source_name: String,
    pub last_event_id: String,
    pub last_created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// ATProto label types
// ---------------------------------------------------------------------------

pub mod atproto_labels;
pub use atproto_labels::*;
