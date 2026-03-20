//! Named queries for the Divine Bridge database.
//!
//! All query functions live here and are re-exported from the crate root.

use anyhow::Result;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Nullable, Text};
use diesel::PgConnection;

use divine_bridge_types::PublishState;

use crate::models::*;
use crate::schema::*;

// ---------------------------------------------------------------------------
// account_links queries
// ---------------------------------------------------------------------------

/// Look up an account link by Nostr pubkey.
pub fn get_account_by_pubkey(conn: &mut PgConnection, pubkey: &str) -> Result<Option<AccountLink>> {
    let result = account_links::table
        .find(pubkey)
        .first::<AccountLink>(conn)
        .optional()?;
    Ok(result)
}

/// Look up an account link by DID.
pub fn get_account_by_did(conn: &mut PgConnection, did: &str) -> Result<Option<AccountLink>> {
    let result = account_links::table
        .filter(account_links::did.eq(Some(did)))
        .first::<AccountLink>(conn)
        .optional()?;
    Ok(result)
}

/// Look up an account link by handle.
pub fn get_account_by_handle(conn: &mut PgConnection, handle: &str) -> Result<Option<AccountLink>> {
    let result = account_links::table
        .filter(account_links::handle.eq(handle))
        .first::<AccountLink>(conn)
        .optional()?;
    Ok(result)
}

/// Insert a new account link. Returns error on conflict (idempotency: check first).
pub fn insert_account_link(conn: &mut PgConnection, link: &NewAccountLink) -> Result<AccountLink> {
    let result = diesel::insert_into(account_links::table)
        .values(link)
        .get_result::<AccountLink>(conn)?;
    Ok(result)
}

/// Look up lifecycle-aware account-link state by pubkey using raw SQL so it can
/// evolve ahead of the generated Diesel schema file.
pub fn get_account_link_lifecycle(
    conn: &mut PgConnection,
    pubkey: &str,
) -> Result<Option<AccountLinkLifecycleRow>> {
    let result = sql_query(
        "SELECT nostr_pubkey, did, handle, crosspost_enabled, signing_key_id, \
         plc_rotation_key_ref, provisioning_state, provisioning_error, disabled_at, \
         created_at, updated_at \
         FROM account_links WHERE nostr_pubkey = $1",
    )
    .bind::<Text, _>(pubkey)
    .get_result::<AccountLinkLifecycleRow>(conn)
    .optional()?;
    Ok(result)
}

/// Insert or update the pending lifecycle state before PLC/PDS side effects.
pub fn upsert_pending_account_link(
    conn: &mut PgConnection,
    nostr_pubkey: &str,
    handle: &str,
    signing_key_id: &str,
    plc_rotation_key_ref: &str,
) -> Result<AccountLinkLifecycleRow> {
    let result = sql_query(
        "INSERT INTO account_links (
            nostr_pubkey, did, handle, crosspost_enabled, signing_key_id,
            plc_rotation_key_ref, provisioning_state, provisioning_error, disabled_at
         ) VALUES ($1, NULL, $2, FALSE, $3, $4, 'pending', NULL, NULL)
         ON CONFLICT (nostr_pubkey) DO UPDATE
         SET handle = EXCLUDED.handle,
             signing_key_id = EXCLUDED.signing_key_id,
             plc_rotation_key_ref = EXCLUDED.plc_rotation_key_ref,
             provisioning_state = 'pending',
             provisioning_error = NULL,
             disabled_at = NULL,
             updated_at = NOW()
         RETURNING nostr_pubkey, did, handle, crosspost_enabled, signing_key_id,
                   plc_rotation_key_ref, provisioning_state, provisioning_error, disabled_at,
                   created_at, updated_at",
    )
    .bind::<Text, _>(nostr_pubkey)
    .bind::<Text, _>(handle)
    .bind::<Text, _>(signing_key_id)
    .bind::<Text, _>(plc_rotation_key_ref)
    .get_result::<AccountLinkLifecycleRow>(conn)?;
    Ok(result)
}

/// Mark a lifecycle record ready after the PDS account exists.
pub fn mark_account_link_ready(
    conn: &mut PgConnection,
    nostr_pubkey: &str,
    did: &str,
) -> Result<AccountLinkLifecycleRow> {
    let result = sql_query(
        "UPDATE account_links
         SET did = $2,
             provisioning_state = 'ready',
             provisioning_error = NULL,
             updated_at = NOW()
         WHERE nostr_pubkey = $1
         RETURNING nostr_pubkey, did, handle, crosspost_enabled, signing_key_id,
                   plc_rotation_key_ref, provisioning_state, provisioning_error, disabled_at,
                   created_at, updated_at",
    )
    .bind::<Text, _>(nostr_pubkey)
    .bind::<Text, _>(did)
    .get_result::<AccountLinkLifecycleRow>(conn)?;
    Ok(result)
}

/// Mark a lifecycle record failed while preserving any created DID.
pub fn mark_account_link_failed(
    conn: &mut PgConnection,
    nostr_pubkey: &str,
    did: Option<&str>,
    error: &str,
) -> Result<AccountLinkLifecycleRow> {
    let result = sql_query(
        "UPDATE account_links
         SET did = COALESCE($2, did),
             provisioning_state = 'failed',
             provisioning_error = $3,
             updated_at = NOW()
         WHERE nostr_pubkey = $1
         RETURNING nostr_pubkey, did, handle, crosspost_enabled, signing_key_id,
                   plc_rotation_key_ref, provisioning_state, provisioning_error, disabled_at,
                   created_at, updated_at",
    )
    .bind::<Text, _>(nostr_pubkey)
    .bind::<Nullable<Text>, _>(did)
    .bind::<Text, _>(error)
    .get_result::<AccountLinkLifecycleRow>(conn)?;
    Ok(result)
}

/// Disable an existing account-link record.
pub fn disable_account_link(
    conn: &mut PgConnection,
    nostr_pubkey: &str,
) -> Result<AccountLinkLifecycleRow> {
    let result = sql_query(
        "UPDATE account_links
         SET provisioning_state = 'disabled',
             disabled_at = NOW(),
             updated_at = NOW()
         WHERE nostr_pubkey = $1
         RETURNING nostr_pubkey, did, handle, crosspost_enabled, signing_key_id,
                   plc_rotation_key_ref, provisioning_state, provisioning_error, disabled_at,
                   created_at, updated_at",
    )
    .bind::<Text, _>(nostr_pubkey)
    .get_result::<AccountLinkLifecycleRow>(conn)?;
    Ok(result)
}

// ---------------------------------------------------------------------------
// ingest_offsets queries
// ---------------------------------------------------------------------------

/// Get the current offset for a named source (relay).
pub fn get_ingest_offset(conn: &mut PgConnection, source: &str) -> Result<Option<IngestOffset>> {
    let result = ingest_offsets::table
        .find(source)
        .first::<IngestOffset>(conn)
        .optional()?;
    Ok(result)
}

/// Upsert the ingest offset for a named source.
pub fn upsert_ingest_offset(
    conn: &mut PgConnection,
    offset: &UpsertIngestOffset,
) -> Result<IngestOffset> {
    let result = diesel::insert_into(ingest_offsets::table)
        .values(offset)
        .on_conflict(ingest_offsets::source_name)
        .do_update()
        .set(offset)
        .get_result::<IngestOffset>(conn)?;
    Ok(result)
}

// ---------------------------------------------------------------------------
// asset_manifest queries
// ---------------------------------------------------------------------------

/// Look up an asset by its source SHA-256 hash (idempotency check).
pub fn get_asset_by_sha256(
    conn: &mut PgConnection,
    sha256: &str,
) -> Result<Option<AssetManifestEntry>> {
    let result = asset_manifest::table
        .find(sha256)
        .first::<AssetManifestEntry>(conn)
        .optional()?;
    Ok(result)
}

/// Insert a new asset manifest entry.
pub fn insert_asset(
    conn: &mut PgConnection,
    entry: &NewAssetManifestEntry,
) -> Result<AssetManifestEntry> {
    let result = diesel::insert_into(asset_manifest::table)
        .values(entry)
        .on_conflict(asset_manifest::source_sha256)
        .do_update()
        .set((
            asset_manifest::blossom_url.eq(entry.blossom_url),
            asset_manifest::at_blob_cid.eq(entry.at_blob_cid),
            asset_manifest::mime.eq(entry.mime),
            asset_manifest::bytes.eq(entry.bytes),
            asset_manifest::is_derivative.eq(entry.is_derivative),
        ))
        .get_result::<AssetManifestEntry>(conn)?;
    Ok(result)
}

// ---------------------------------------------------------------------------
// record_mappings queries
// ---------------------------------------------------------------------------

/// Check if a Nostr event has already been bridged (idempotency).
pub fn get_record_mapping(
    conn: &mut PgConnection,
    nostr_event_id: &str,
) -> Result<Option<RecordMapping>> {
    let result = record_mappings::table
        .find(nostr_event_id)
        .first::<RecordMapping>(conn)
        .optional()?;
    Ok(result)
}

/// Look up a record mapping by AT URI.
pub fn get_record_by_at_uri(
    conn: &mut PgConnection,
    at_uri: &str,
) -> Result<Option<RecordMapping>> {
    let result = record_mappings::table
        .filter(record_mappings::at_uri.eq(at_uri))
        .first::<RecordMapping>(conn)
        .optional()?;
    Ok(result)
}

/// Insert a new record mapping.
pub fn insert_record_mapping(
    conn: &mut PgConnection,
    mapping: &NewRecordMapping,
) -> Result<RecordMapping> {
    let result = diesel::insert_into(record_mappings::table)
        .values(mapping)
        .on_conflict(record_mappings::nostr_event_id)
        .do_update()
        .set((
            record_mappings::did.eq(mapping.did),
            record_mappings::collection.eq(mapping.collection),
            record_mappings::rkey.eq(mapping.rkey),
            record_mappings::at_uri.eq(mapping.at_uri),
            record_mappings::cid.eq(mapping.cid),
            record_mappings::status.eq(mapping.status),
        ))
        .get_result::<RecordMapping>(conn)?;
    Ok(result)
}

/// Update record mapping status and optional CID.
pub fn update_record_mapping_status(
    conn: &mut PgConnection,
    nostr_event_id: &str,
    cid: Option<&str>,
    status: &str,
) -> Result<RecordMapping> {
    let result = diesel::update(record_mappings::table.find(nostr_event_id))
        .set((
            record_mappings::cid.eq(cid),
            record_mappings::status.eq(status),
        ))
        .get_result::<RecordMapping>(conn)?;
    Ok(result)
}

// ---------------------------------------------------------------------------
// moderation_actions queries
// ---------------------------------------------------------------------------

/// Insert a new moderation action.
pub fn insert_moderation_action(
    conn: &mut PgConnection,
    action: &NewModerationAction,
) -> Result<ModerationActionRow> {
    let result = diesel::insert_into(moderation_actions::table)
        .values(action)
        .get_result::<ModerationActionRow>(conn)?;
    Ok(result)
}

/// Update a moderation action's state.
pub fn update_moderation_action_state(
    conn: &mut PgConnection,
    action_id: i64,
    new_state: &str,
) -> Result<ModerationActionRow> {
    let result = diesel::update(moderation_actions::table.find(action_id))
        .set(moderation_actions::state.eq(new_state))
        .get_result::<ModerationActionRow>(conn)?;
    Ok(result)
}

// ---------------------------------------------------------------------------
// publish_jobs queries
// ---------------------------------------------------------------------------

/// Get a publish job by Nostr event ID (idempotency check).
pub fn get_publish_job(
    conn: &mut PgConnection,
    nostr_event_id: &str,
) -> Result<Option<PublishJob>> {
    let result = publish_jobs::table
        .find(nostr_event_id)
        .first::<PublishJob>(conn)
        .optional()?;
    Ok(result)
}

/// Get pending publish jobs, ordered by creation time.
pub fn get_pending_jobs(conn: &mut PgConnection, limit: i64) -> Result<Vec<PublishJob>> {
    let results = publish_jobs::table
        .filter(publish_jobs::state.eq(PublishState::Pending.as_str()))
        .order(publish_jobs::created_at.asc())
        .limit(limit)
        .load::<PublishJob>(conn)?;
    Ok(results)
}

/// Insert a new publish job.
pub fn insert_publish_job(conn: &mut PgConnection, job: &NewPublishJob) -> Result<PublishJob> {
    let result = diesel::insert_into(publish_jobs::table)
        .values(job)
        .get_result::<PublishJob>(conn)?;
    Ok(result)
}

/// Update a publish job's state and attempt count.
pub fn update_publish_job_state(
    conn: &mut PgConnection,
    nostr_event_id: &str,
    new_state: PublishState,
    new_attempt: i32,
    error_msg: Option<&str>,
) -> Result<PublishJob> {
    let result = diesel::update(publish_jobs::table.find(nostr_event_id))
        .set((
            publish_jobs::state.eq(new_state.as_str()),
            publish_jobs::attempt.eq(new_attempt),
            publish_jobs::error.eq(error_msg),
            publish_jobs::updated_at.eq(diesel::dsl::now),
        ))
        .get_result::<PublishJob>(conn)?;
    Ok(result)
}

// ---------------------------------------------------------------------------
// labeler_events queries
// ---------------------------------------------------------------------------

pub fn insert_labeler_event(
    conn: &mut PgConnection,
    event: &NewLabelerEvent,
) -> Result<LabelerEvent> {
    let result = diesel::insert_into(labeler_events::table)
        .values(event)
        .get_result::<LabelerEvent>(conn)?;
    Ok(result)
}

pub fn get_labeler_events_after(
    conn: &mut PgConnection,
    after_seq: i64,
    limit: i64,
) -> Result<Vec<LabelerEvent>> {
    let results = labeler_events::table
        .filter(labeler_events::seq.gt(after_seq))
        .order(labeler_events::seq.asc())
        .limit(limit)
        .load::<LabelerEvent>(conn)?;
    Ok(results)
}

pub fn get_latest_labeler_seq(conn: &mut PgConnection) -> Result<Option<i64>> {
    use diesel::dsl::max;
    let result = labeler_events::table
        .select(max(labeler_events::seq))
        .first::<Option<i64>>(conn)?;
    Ok(result)
}

// ---------------------------------------------------------------------------
// inbound_labels queries
// ---------------------------------------------------------------------------

pub fn insert_inbound_label(
    conn: &mut PgConnection,
    label: &NewInboundLabel,
) -> Result<InboundLabel> {
    let result = diesel::insert_into(inbound_labels::table)
        .values(label)
        .get_result::<InboundLabel>(conn)?;
    Ok(result)
}

pub fn get_pending_inbound_labels(
    conn: &mut PgConnection,
    limit: i64,
) -> Result<Vec<InboundLabel>> {
    let results = inbound_labels::table
        .filter(inbound_labels::review_state.eq("pending"))
        .order(inbound_labels::created_at.asc())
        .limit(limit)
        .load::<InboundLabel>(conn)?;
    Ok(results)
}

pub fn update_inbound_label_review(
    conn: &mut PgConnection,
    label_id: i64,
    new_state: &str,
    reviewer: &str,
) -> Result<InboundLabel> {
    let now = chrono::Utc::now();
    let result = diesel::update(inbound_labels::table.find(label_id))
        .set((
            inbound_labels::review_state.eq(new_state),
            inbound_labels::reviewed_by.eq(Some(reviewer)),
            inbound_labels::reviewed_at.eq(Some(now)),
        ))
        .get_result::<InboundLabel>(conn)?;
    Ok(result)
}
