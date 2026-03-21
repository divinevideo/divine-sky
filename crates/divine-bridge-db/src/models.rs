//! Diesel models for the 6 bridge tables.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_types::{Bool, Int4, Int8, Nullable, Text, Timestamptz};

use crate::schema::*;

// ---------------------------------------------------------------------------
// account_links
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = account_links)]
#[diesel(primary_key(nostr_pubkey))]
pub struct AccountLink {
    pub nostr_pubkey: String,
    pub did: Option<String>,
    pub handle: String,
    pub crosspost_enabled: bool,
    pub signing_key_id: String,
    pub plc_rotation_key_ref: String,
    pub provisioning_state: String,
    pub provisioning_error: Option<String>,
    pub disabled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = account_links)]
pub struct NewAccountLink<'a> {
    pub nostr_pubkey: &'a str,
    pub did: Option<&'a str>,
    pub handle: &'a str,
    pub crosspost_enabled: bool,
    pub signing_key_id: &'a str,
    pub plc_rotation_key_ref: &'a str,
    pub provisioning_state: &'a str,
    pub provisioning_error: Option<&'a str>,
    pub disabled_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvisioningState {
    Pending,
    Ready,
    Failed,
    Disabled,
}

impl ProvisioningState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone, QueryableByName)]
pub struct AccountLinkLifecycleRow {
    #[diesel(sql_type = Text)]
    pub nostr_pubkey: String,
    #[diesel(sql_type = Nullable<Text>)]
    pub did: Option<String>,
    #[diesel(sql_type = Text)]
    pub handle: String,
    #[diesel(sql_type = Bool)]
    pub crosspost_enabled: bool,
    #[diesel(sql_type = Nullable<Text>)]
    pub signing_key_id: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    pub plc_rotation_key_ref: Option<String>,
    #[diesel(sql_type = Text)]
    pub provisioning_state: String,
    #[diesel(sql_type = Nullable<Text>)]
    pub provisioning_error: Option<String>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    pub disabled_at: Option<DateTime<Utc>>,
    #[diesel(sql_type = Timestamptz)]
    pub created_at: DateTime<Utc>,
    #[diesel(sql_type = Timestamptz)]
    pub updated_at: DateTime<Utc>,
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

// ---------------------------------------------------------------------------
// labeler_events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = labeler_events)]
#[diesel(primary_key(seq))]
pub struct LabelerEvent {
    pub seq: i64,
    pub src_did: String,
    pub subject_uri: String,
    pub subject_cid: Option<String>,
    pub val: String,
    pub neg: bool,
    pub nostr_event_id: Option<String>,
    pub sha256: Option<String>,
    pub origin: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = labeler_events)]
pub struct NewLabelerEvent<'a> {
    pub src_did: &'a str,
    pub subject_uri: &'a str,
    pub subject_cid: Option<&'a str>,
    pub val: &'a str,
    pub neg: bool,
    pub nostr_event_id: Option<&'a str>,
    pub sha256: Option<&'a str>,
    pub origin: &'a str,
}

// ---------------------------------------------------------------------------
// inbound_labels
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = inbound_labels)]
pub struct InboundLabel {
    pub id: i64,
    pub labeler_did: String,
    pub subject_uri: String,
    pub val: String,
    pub neg: bool,
    pub nostr_event_id: Option<String>,
    pub sha256: Option<String>,
    pub divine_label: Option<String>,
    pub review_state: String,
    pub reviewed_by: Option<String>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub raw_json: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = inbound_labels)]
pub struct NewInboundLabel<'a> {
    pub labeler_did: &'a str,
    pub subject_uri: &'a str,
    pub val: &'a str,
    pub neg: bool,
    pub nostr_event_id: Option<&'a str>,
    pub sha256: Option<&'a str>,
    pub divine_label: Option<&'a str>,
    pub review_state: &'a str,
    pub raw_json: Option<&'a str>,
}

// ---------------------------------------------------------------------------
// appview_repos
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = appview_repos)]
#[diesel(primary_key(did))]
pub struct AppviewRepo {
    pub did: String,
    pub handle: Option<String>,
    pub head: Option<String>,
    pub rev: Option<String>,
    pub active: bool,
    pub last_backfilled_at: Option<DateTime<Utc>>,
    pub last_seen_seq: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = appview_repos)]
pub struct NewAppviewRepo<'a> {
    pub did: &'a str,
    pub handle: Option<&'a str>,
    pub head: Option<&'a str>,
    pub rev: Option<&'a str>,
    pub active: bool,
    pub last_backfilled_at: Option<DateTime<Utc>>,
    pub last_seen_seq: Option<i64>,
}

// ---------------------------------------------------------------------------
// appview_profiles
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = appview_profiles)]
#[diesel(primary_key(did))]
pub struct AppviewProfile {
    pub did: String,
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub website: Option<String>,
    pub avatar_cid: Option<String>,
    pub banner_cid: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub raw_json: Option<String>,
    pub indexed_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = appview_profiles)]
pub struct NewAppviewProfile<'a> {
    pub did: &'a str,
    pub handle: Option<&'a str>,
    pub display_name: Option<&'a str>,
    pub description: Option<&'a str>,
    pub website: Option<&'a str>,
    pub avatar_cid: Option<&'a str>,
    pub banner_cid: Option<&'a str>,
    pub created_at: Option<DateTime<Utc>>,
    pub raw_json: Option<&'a str>,
    pub indexed_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// appview_posts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = appview_posts)]
#[diesel(primary_key(uri))]
pub struct AppviewPost {
    pub uri: String,
    pub did: String,
    pub rkey: String,
    pub record_cid: Option<String>,
    pub created_at: DateTime<Utc>,
    pub text: String,
    pub langs_json: Option<String>,
    pub embed_blob_cid: Option<String>,
    pub embed_alt: Option<String>,
    pub aspect_ratio_width: Option<i32>,
    pub aspect_ratio_height: Option<i32>,
    pub raw_json: Option<String>,
    pub search_text: String,
    pub indexed_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = appview_posts)]
pub struct NewAppviewPost<'a> {
    pub uri: &'a str,
    pub did: &'a str,
    pub rkey: &'a str,
    pub record_cid: Option<&'a str>,
    pub created_at: DateTime<Utc>,
    pub text: &'a str,
    pub langs_json: Option<&'a str>,
    pub embed_blob_cid: Option<&'a str>,
    pub embed_alt: Option<&'a str>,
    pub aspect_ratio_width: Option<i32>,
    pub aspect_ratio_height: Option<i32>,
    pub raw_json: Option<&'a str>,
    pub search_text: &'a str,
    pub indexed_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// appview_media_views
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = appview_media_views)]
#[diesel(primary_key(did, blob_cid))]
pub struct AppviewMediaView {
    pub did: String,
    pub blob_cid: String,
    pub playlist_url: String,
    pub thumbnail_url: Option<String>,
    pub mime_type: String,
    pub bytes: i64,
    pub ready: bool,
    pub last_derived_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = appview_media_views)]
pub struct NewAppviewMediaView<'a> {
    pub did: &'a str,
    pub blob_cid: &'a str,
    pub playlist_url: &'a str,
    pub thumbnail_url: Option<&'a str>,
    pub mime_type: &'a str,
    pub bytes: i64,
    pub ready: bool,
    pub last_derived_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// appview_service_state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = appview_service_state)]
#[diesel(primary_key(state_key))]
pub struct AppviewServiceState {
    pub state_key: String,
    pub state_value: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = appview_service_state)]
pub struct NewAppviewServiceState<'a> {
    pub state_key: &'a str,
    pub state_value: Option<&'a str>,
}

#[derive(Debug, Clone, QueryableByName)]
pub struct AppviewPostWithMediaViewRow {
    #[diesel(sql_type = Text)]
    pub uri: String,
    #[diesel(sql_type = Text)]
    pub did: String,
    #[diesel(sql_type = Text)]
    pub rkey: String,
    #[diesel(sql_type = Nullable<Text>)]
    pub record_cid: Option<String>,
    #[diesel(sql_type = Timestamptz)]
    pub created_at: DateTime<Utc>,
    #[diesel(sql_type = Text)]
    pub text: String,
    #[diesel(sql_type = Nullable<Text>)]
    pub langs_json: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    pub embed_blob_cid: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    pub embed_alt: Option<String>,
    #[diesel(sql_type = Nullable<Int4>)]
    pub aspect_ratio_width: Option<i32>,
    #[diesel(sql_type = Nullable<Int4>)]
    pub aspect_ratio_height: Option<i32>,
    #[diesel(sql_type = Nullable<Text>)]
    pub raw_json: Option<String>,
    #[diesel(sql_type = Text)]
    pub search_text: String,
    #[diesel(sql_type = Timestamptz)]
    pub indexed_at: DateTime<Utc>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    pub deleted_at: Option<DateTime<Utc>>,
    #[diesel(sql_type = Nullable<Text>)]
    pub playlist_url: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    pub thumbnail_url: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    pub media_mime_type: Option<String>,
    #[diesel(sql_type = Nullable<Int8>)]
    pub media_bytes: Option<i64>,
    #[diesel(sql_type = Nullable<Bool>)]
    pub media_ready: Option<bool>,
}
