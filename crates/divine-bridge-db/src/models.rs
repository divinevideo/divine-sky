//! Diesel models for the 6 bridge tables.

use chrono::{DateTime, Utc};
use diesel::prelude::*;

use crate::schema::*;

// ---------------------------------------------------------------------------
// account_links
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = account_links)]
#[diesel(primary_key(nostr_pubkey))]
pub struct AccountLink {
    pub nostr_pubkey: String,
    pub did: String,
    pub handle: String,
    pub crosspost_enabled: bool,
    pub signing_key_id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = account_links)]
pub struct NewAccountLink<'a> {
    pub nostr_pubkey: &'a str,
    pub did: &'a str,
    pub handle: &'a str,
    pub crosspost_enabled: bool,
    pub signing_key_id: &'a str,
}

// ---------------------------------------------------------------------------
// ingest_offsets
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = ingest_offsets)]
#[diesel(primary_key(source_name))]
pub struct IngestOffset {
    pub source_name: String,
    pub last_event_id: String,
    pub last_created_at: DateTime<Utc>,
}

#[derive(Debug, Insertable, AsChangeset)]
#[diesel(table_name = ingest_offsets)]
pub struct UpsertIngestOffset<'a> {
    pub source_name: &'a str,
    pub last_event_id: &'a str,
    pub last_created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// asset_manifest
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = asset_manifest)]
#[diesel(primary_key(source_sha256))]
pub struct AssetManifestEntry {
    pub source_sha256: String,
    pub blossom_url: Option<String>,
    pub at_blob_cid: String,
    pub mime: String,
    pub bytes: i64,
    pub is_derivative: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = asset_manifest)]
pub struct NewAssetManifestEntry<'a> {
    pub source_sha256: &'a str,
    pub blossom_url: Option<&'a str>,
    pub at_blob_cid: &'a str,
    pub mime: &'a str,
    pub bytes: i64,
    pub is_derivative: bool,
}

// ---------------------------------------------------------------------------
// record_mappings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Associations)]
#[diesel(table_name = record_mappings)]
#[diesel(primary_key(nostr_event_id))]
#[diesel(belongs_to(AccountLink, foreign_key = did))]
pub struct RecordMapping {
    pub nostr_event_id: String,
    pub did: String,
    pub collection: String,
    pub rkey: String,
    pub at_uri: String,
    pub cid: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = record_mappings)]
pub struct NewRecordMapping<'a> {
    pub nostr_event_id: &'a str,
    pub did: &'a str,
    pub collection: &'a str,
    pub rkey: &'a str,
    pub at_uri: &'a str,
    pub cid: Option<&'a str>,
    pub status: &'a str,
}

// ---------------------------------------------------------------------------
// moderation_actions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = moderation_actions)]
pub struct ModerationActionRow {
    pub id: i64,
    pub subject_type: String,
    pub subject_id: String,
    pub action: String,
    pub origin: String,
    pub reason: Option<String>,
    pub state: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = moderation_actions)]
pub struct NewModerationAction<'a> {
    pub subject_type: &'a str,
    pub subject_id: &'a str,
    pub action: &'a str,
    pub origin: &'a str,
    pub reason: Option<&'a str>,
    pub state: &'a str,
}

// ---------------------------------------------------------------------------
// publish_jobs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = publish_jobs)]
#[diesel(primary_key(nostr_event_id))]
pub struct PublishJob {
    pub nostr_event_id: String,
    pub attempt: i32,
    pub state: String,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = publish_jobs)]
pub struct NewPublishJob<'a> {
    pub nostr_event_id: &'a str,
    pub state: &'a str,
}
