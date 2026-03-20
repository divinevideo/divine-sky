//! Named queries for the Divine Bridge database.
//!
//! All query functions live here and are re-exported from the crate root.

use anyhow::Result;
use diesel::prelude::*;
use diesel::PgConnection;

use divine_bridge_types::PublishState;

use crate::models::*;
use crate::schema::*;

// ---------------------------------------------------------------------------
// account_links queries
// ---------------------------------------------------------------------------

/// Look up an account link by Nostr pubkey.
pub fn get_account_by_pubkey(
    conn: &mut PgConnection,
    pubkey: &str,
) -> Result<Option<AccountLink>> {
    let result = account_links::table
        .find(pubkey)
        .first::<AccountLink>(conn)
        .optional()?;
    Ok(result)
}

/// Look up an account link by DID.
pub fn get_account_by_did(conn: &mut PgConnection, did: &str) -> Result<Option<AccountLink>> {
    let result = account_links::table
        .filter(account_links::did.eq(did))
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

// ---------------------------------------------------------------------------
// ingest_offsets queries
// ---------------------------------------------------------------------------

/// Get the current offset for a named source (relay).
pub fn get_ingest_offset(
    conn: &mut PgConnection,
    source: &str,
) -> Result<Option<IngestOffset>> {
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
pub fn insert_publish_job(
    conn: &mut PgConnection,
    job: &NewPublishJob,
) -> Result<PublishJob> {
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
