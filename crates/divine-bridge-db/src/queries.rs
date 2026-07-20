//! Named queries for the Divine Bridge database.
//!
//! All query functions live here and are re-exported from the crate root.

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Array, BigInt, Bool, Int8, Jsonb, Nullable, Text, Timestamptz};
use diesel::PgConnection;
use diesel::PgTextExpressionMethods;

use divine_bridge_types::{PublishJobSource, PublishState};

use crate::models::*;
use crate::schema::*;

const ACCOUNT_LINK_LIFECYCLE_COLUMNS: &str = "nostr_pubkey, did, handle, crosspost_enabled, \
    signing_key_id, plc_rotation_key_ref, provisioning_state, provisioning_error, \
    publish_backfill_state, publish_backfill_started_at, publish_backfill_completed_at, \
    publish_backfill_error, disabled_at, created_at, updated_at";

#[derive(Debug, QueryableByName)]
struct PublishJobIdRow {
    #[diesel(sql_type = Text)]
    nostr_event_id: String,
}

#[derive(Debug, QueryableByName)]
struct ReservedRkeyRow {
    #[diesel(sql_type = Text)]
    reserved_rkey: String,
}

#[derive(Debug, QueryableByName)]
struct PreparedRecordRow {
    #[diesel(sql_type = Jsonb)]
    prepared_record: serde_json::Value,
}

#[derive(Debug, QueryableByName)]
struct BooleanRow {
    #[diesel(sql_type = Bool)]
    value: bool,
}

#[derive(Debug, QueryableByName)]
struct LegacyRepairCountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

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
         plc_rotation_key_ref, provisioning_state, provisioning_error, \
         publish_backfill_state, publish_backfill_started_at, \
         publish_backfill_completed_at, publish_backfill_error, disabled_at, \
         created_at, updated_at \
         FROM account_links WHERE nostr_pubkey = $1",
    )
    .bind::<Text, _>(pubkey)
    .get_result::<AccountLinkLifecycleRow>(conn)
    .optional()?;
    Ok(result)
}

/// Look up lifecycle-aware account-link state by handle.
pub fn get_account_link_lifecycle_by_handle(
    conn: &mut PgConnection,
    handle: &str,
) -> Result<Option<AccountLinkLifecycleRow>> {
    let result = sql_query(
        "SELECT nostr_pubkey, did, handle, crosspost_enabled, signing_key_id, \
         plc_rotation_key_ref, provisioning_state, provisioning_error, \
         publish_backfill_state, publish_backfill_started_at, \
         publish_backfill_completed_at, publish_backfill_error, disabled_at, \
         created_at, updated_at \
         FROM account_links WHERE handle = $1",
    )
    .bind::<Text, _>(handle)
    .get_result::<AccountLinkLifecycleRow>(conn)
    .optional()?;
    Ok(result)
}

/// Load persisted non-pending lifecycle rows that should be republished on startup.
pub fn list_account_link_lifecycle_for_reconciliation(
    conn: &mut PgConnection,
) -> Result<Vec<AccountLinkLifecycleRow>> {
    let rows = sql_query(
        "SELECT nostr_pubkey, did, handle, crosspost_enabled, signing_key_id, \
         plc_rotation_key_ref, provisioning_state, provisioning_error, \
         publish_backfill_state, publish_backfill_started_at, \
         publish_backfill_completed_at, publish_backfill_error, disabled_at, \
         created_at, updated_at \
         FROM account_links
         WHERE provisioning_state IN ('ready', 'failed', 'disabled')
         ORDER BY created_at ASC",
    )
    .load::<AccountLinkLifecycleRow>(conn)?;
    Ok(rows)
}

/// Insert or update the pending lifecycle state before PLC/PDS side effects.
pub fn upsert_pending_account_link(
    conn: &mut PgConnection,
    nostr_pubkey: &str,
    handle: &str,
    signing_key_id: &str,
    plc_rotation_key_ref: &str,
    crosspost_enabled: bool,
) -> Result<AccountLinkLifecycleRow> {
    let result = sql_query(
        "INSERT INTO account_links (
            nostr_pubkey, did, handle, crosspost_enabled, signing_key_id,
            plc_rotation_key_ref, provisioning_state, provisioning_error, disabled_at
         ) VALUES ($1, NULL, $2, $5, $3, $4, 'pending', NULL, NULL)
         ON CONFLICT (nostr_pubkey) DO UPDATE
         SET handle = EXCLUDED.handle,
             crosspost_enabled = EXCLUDED.crosspost_enabled,
             signing_key_id = EXCLUDED.signing_key_id,
             plc_rotation_key_ref = EXCLUDED.plc_rotation_key_ref,
             provisioning_state = 'pending',
             provisioning_error = NULL,
             disabled_at = NULL,
             updated_at = NOW()
         RETURNING nostr_pubkey, did, handle, crosspost_enabled, signing_key_id,
                   plc_rotation_key_ref, provisioning_state, provisioning_error,
                   publish_backfill_state, publish_backfill_started_at,
                   publish_backfill_completed_at, publish_backfill_error, disabled_at,
                   created_at, updated_at",
    )
    .bind::<Text, _>(nostr_pubkey)
    .bind::<Text, _>(handle)
    .bind::<Text, _>(signing_key_id)
    .bind::<Text, _>(plc_rotation_key_ref)
    .bind::<diesel::sql_types::Bool, _>(crosspost_enabled)
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
                   plc_rotation_key_ref, provisioning_state, provisioning_error,
                   publish_backfill_state, publish_backfill_started_at,
                   publish_backfill_completed_at, publish_backfill_error, disabled_at,
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
                   plc_rotation_key_ref, provisioning_state, provisioning_error,
                   publish_backfill_state, publish_backfill_started_at,
                   publish_backfill_completed_at, publish_backfill_error, disabled_at,
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
         SET crosspost_enabled = FALSE,
             provisioning_state = 'disabled',
             disabled_at = NOW(),
             updated_at = NOW()
         WHERE nostr_pubkey = $1
         RETURNING nostr_pubkey, did, handle, crosspost_enabled, signing_key_id,
                   plc_rotation_key_ref, provisioning_state, provisioning_error,
                   publish_backfill_state, publish_backfill_started_at,
                   publish_backfill_completed_at, publish_backfill_error, disabled_at,
                   created_at, updated_at",
    )
    .bind::<Text, _>(nostr_pubkey)
    .get_result::<AccountLinkLifecycleRow>(conn)?;
    Ok(result)
}

#[derive(Debug, QueryableByName)]
struct PdsAccessJwtRow {
    #[diesel(sql_type = diesel::sql_types::Nullable<Text>)]
    pds_access_jwt: Option<String>,
}

/// Look up the stored PDS access JWT for an account by its DID (for repo writes).
pub fn get_account_pds_access_jwt_by_did(
    conn: &mut PgConnection,
    did: &str,
) -> Result<Option<String>> {
    let row = sql_query("SELECT pds_access_jwt FROM account_links WHERE did = $1")
        .bind::<Text, _>(did)
        .get_result::<PdsAccessJwtRow>(conn)
        .optional()?;
    Ok(row.and_then(|r| r.pds_access_jwt))
}

#[derive(Debug, QueryableByName)]
struct PdsRefreshJwtRow {
    #[diesel(sql_type = diesel::sql_types::Nullable<Text>)]
    pds_refresh_jwt: Option<String>,
}

/// Look up the stored PDS refresh JWT for an account by its DID (for refreshSession).
pub fn get_account_pds_refresh_jwt_by_did(
    conn: &mut PgConnection,
    did: &str,
) -> Result<Option<String>> {
    let row = sql_query("SELECT pds_refresh_jwt FROM account_links WHERE did = $1")
        .bind::<Text, _>(did)
        .get_result::<PdsRefreshJwtRow>(conn)
        .optional()?;
    Ok(row.and_then(|r| r.pds_refresh_jwt))
}

/// Persist a rotated PDS session, keyed by DID (used after refreshSession).
pub fn store_account_pds_session_by_did(
    conn: &mut PgConnection,
    did: &str,
    access_jwt: &str,
    refresh_jwt: &str,
) -> Result<()> {
    sql_query(
        "UPDATE account_links
         SET pds_access_jwt = $2,
             pds_refresh_jwt = $3,
             pds_session_updated_at = NOW(),
             updated_at = NOW()
         WHERE did = $1",
    )
    .bind::<Text, _>(did)
    .bind::<Text, _>(access_jwt)
    .bind::<Text, _>(refresh_jwt)
    .execute(conn)?;
    Ok(())
}

/// Persist the account's PDS session (access/refresh JWT) for later repo writes.
pub fn store_account_pds_session(
    conn: &mut PgConnection,
    nostr_pubkey: &str,
    access_jwt: &str,
    refresh_jwt: &str,
) -> Result<()> {
    sql_query(
        "UPDATE account_links
         SET pds_access_jwt = $2,
             pds_refresh_jwt = $3,
             pds_session_updated_at = NOW(),
             updated_at = NOW()
         WHERE nostr_pubkey = $1",
    )
    .bind::<Text, _>(nostr_pubkey)
    .bind::<Text, _>(access_jwt)
    .bind::<Text, _>(refresh_jwt)
    .execute(conn)?;
    Ok(())
}

/// Re-enable a previously disabled account-link record.
///
/// Restores `provisioning_state` to `ready` if the account has a DID
/// (was previously provisioned), or `pending` if not.
pub fn enable_account_link(
    conn: &mut PgConnection,
    nostr_pubkey: &str,
) -> Result<AccountLinkLifecycleRow> {
    let query = format!(
        "UPDATE account_links
         SET crosspost_enabled = TRUE,
             provisioning_state = CASE
                 WHEN did IS NOT NULL THEN 'ready'
                 ELSE 'pending'
             END,
             disabled_at = NULL,
             updated_at = NOW()
         WHERE nostr_pubkey = $1
         RETURNING {ACCOUNT_LINK_LIFECYCLE_COLUMNS}"
    );
    let result = sql_query(query)
        .bind::<Text, _>(nostr_pubkey)
        .get_result::<AccountLinkLifecycleRow>(conn)?;
    Ok(result)
}

/// Load eligible accounts that still need backlog seeding.
pub fn list_accounts_requiring_backfill(
    conn: &mut PgConnection,
    limit: i64,
) -> Result<Vec<AccountLinkLifecycleRow>> {
    let query = format!(
        "SELECT {ACCOUNT_LINK_LIFECYCLE_COLUMNS}
         FROM account_links
         WHERE crosspost_enabled = TRUE
           AND provisioning_state = 'ready'
           AND disabled_at IS NULL
           AND publish_backfill_state IN ('not_started', 'failed')
         ORDER BY created_at ASC
         LIMIT $1"
    );
    let rows = sql_query(query)
        .bind::<Int8, _>(limit)
        .load::<AccountLinkLifecycleRow>(conn)?;
    Ok(rows)
}

/// Mark an account backlog as in progress.
pub fn mark_account_backfill_started(
    conn: &mut PgConnection,
    nostr_pubkey: &str,
) -> Result<AccountLinkLifecycleRow> {
    let query = format!(
        "UPDATE account_links
         SET publish_backfill_state = 'in_progress',
             publish_backfill_started_at = NOW(),
             publish_backfill_completed_at = NULL,
             publish_backfill_error = NULL,
             updated_at = NOW()
         WHERE nostr_pubkey = $1
         RETURNING {ACCOUNT_LINK_LIFECYCLE_COLUMNS}"
    );
    let row = sql_query(query)
        .bind::<Text, _>(nostr_pubkey)
        .get_result::<AccountLinkLifecycleRow>(conn)?;
    Ok(row)
}

/// Mark an account backlog as completed.
pub fn mark_account_backfill_completed(
    conn: &mut PgConnection,
    nostr_pubkey: &str,
) -> Result<AccountLinkLifecycleRow> {
    let query = format!(
        "UPDATE account_links
         SET publish_backfill_state = 'completed',
             publish_backfill_completed_at = NOW(),
             publish_backfill_error = NULL,
             updated_at = NOW()
         WHERE nostr_pubkey = $1
         RETURNING {ACCOUNT_LINK_LIFECYCLE_COLUMNS}"
    );
    let row = sql_query(query)
        .bind::<Text, _>(nostr_pubkey)
        .get_result::<AccountLinkLifecycleRow>(conn)?;
    Ok(row)
}

/// Mark an account backlog as failed.
pub fn mark_account_backfill_failed(
    conn: &mut PgConnection,
    nostr_pubkey: &str,
    error: &str,
) -> Result<AccountLinkLifecycleRow> {
    let query = format!(
        "UPDATE account_links
         SET publish_backfill_state = 'failed',
             publish_backfill_completed_at = NULL,
             publish_backfill_error = $2,
             updated_at = NOW()
         WHERE nostr_pubkey = $1
         RETURNING {ACCOUNT_LINK_LIFECYCLE_COLUMNS}"
    );
    let row = sql_query(query)
        .bind::<Text, _>(nostr_pubkey)
        .bind::<Text, _>(error)
        .get_result::<AccountLinkLifecycleRow>(conn)?;
    Ok(row)
}

// ---------------------------------------------------------------------------
// provisioning_keys queries
// ---------------------------------------------------------------------------

/// Look up a persisted provisioning key by its stable reference.
pub fn get_provisioning_key(
    conn: &mut PgConnection,
    key_ref: &str,
) -> Result<Option<ProvisioningKey>> {
    let result = provisioning_keys::table
        .find(key_ref)
        .first::<ProvisioningKey>(conn)
        .optional()?;
    Ok(result)
}

/// Persist a new provisioning key envelope.
pub fn insert_provisioning_key(
    conn: &mut PgConnection,
    key: &NewProvisioningKey<'_>,
) -> Result<ProvisioningKey> {
    let result = diesel::insert_into(provisioning_keys::table)
        .values(key)
        .get_result::<ProvisioningKey>(conn)?;
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

const LEGACY_REPAIR_PREDICATE: &str = "
    FROM publish_jobs p
    INNER JOIN account_links a ON a.nostr_pubkey = p.nostr_pubkey
    WHERE p.nostr_pubkey = $1
      AND a.provisioning_state = 'ready'
      AND a.crosspost_enabled = TRUE
      AND p.state = 'failed'
      AND p.completed_at IS NOT NULL
      AND p.lease_owner IS NULL
      AND (p.lease_expires_at IS NULL OR p.lease_expires_at <= NOW())
      AND (
        (cardinality($2::text[]) > 0 AND p.nostr_event_id = ANY($2))
        OR (
          $3::text IS NOT NULL
          AND (
            p.error = $3
            OR (
              $3 = 'BadJwt: Signature tag didn''t verify'
              AND POSITION('getServiceAuth failed (400)' IN p.error) > 0
              AND POSITION('BadJwt: Signature tag didn''t verify' IN p.error) > 0
            )
          )
        )
      )";

/// Preview an exact, bounded page of terminal legacy BadJwt jobs.
pub fn preview_legacy_badjwt_repair(
    conn: &mut PgConnection,
    filter: &LegacyBadJwtRepairFilter,
) -> Result<LegacyBadJwtRepairPreview> {
    preview_legacy_badjwt_repair_impl(conn, filter, false)
}

/// Preview and lock the selected publish jobs for an audited confirmation.
pub fn lock_legacy_badjwt_repair_candidates(
    conn: &mut PgConnection,
    filter: &LegacyBadJwtRepairFilter,
) -> Result<LegacyBadJwtRepairPreview> {
    preview_legacy_badjwt_repair_impl(conn, filter, true)
}

fn preview_legacy_badjwt_repair_impl(
    conn: &mut PgConnection,
    filter: &LegacyBadJwtRepairFilter,
    lock_candidates: bool,
) -> Result<LegacyBadJwtRepairPreview> {
    validate_legacy_repair_filter(filter)?;

    let count_sql = format!("SELECT COUNT(*) AS count {LEGACY_REPAIR_PREDICATE}");
    let total_matching = sql_query(count_sql)
        .bind::<Text, _>(&filter.nostr_pubkey)
        .bind::<Array<Text>, _>(&filter.event_ids)
        .bind::<Nullable<Text>, _>(filter.exact_error.as_deref())
        .get_result::<LegacyRepairCountRow>(conn)?
        .count;

    let row_lock = if lock_candidates {
        " FOR UPDATE OF p"
    } else {
        ""
    };
    let page_sql = format!(
        "SELECT p.nostr_event_id, p.state, p.attempt, p.error, p.lease_owner, \
                p.lease_expires_at, p.completed_at, p.updated_at \
         {LEGACY_REPAIR_PREDICATE} \
           AND ($4::text IS NULL OR p.nostr_event_id > $4) \
         ORDER BY p.nostr_event_id ASC \
         LIMIT $5{row_lock}"
    );
    let mut jobs = sql_query(page_sql)
        .bind::<Text, _>(&filter.nostr_pubkey)
        .bind::<Array<Text>, _>(&filter.event_ids)
        .bind::<Nullable<Text>, _>(filter.exact_error.as_deref())
        .bind::<Nullable<Text>, _>(filter.after_event_id.as_deref())
        .bind::<BigInt, _>(filter.limit + 1)
        .load::<LegacyRepairJobSnapshot>(conn)?;
    let has_more = jobs.len() as i64 > filter.limit;
    if has_more {
        jobs.truncate(filter.limit as usize);
    }
    let next_after_event_id = has_more
        .then(|| jobs.last().map(|job| job.nostr_event_id.clone()))
        .flatten();

    Ok(LegacyBadJwtRepairPreview {
        jobs,
        total_matching,
        has_more,
        next_after_event_id,
    })
}

fn validate_legacy_repair_filter(filter: &LegacyBadJwtRepairFilter) -> Result<()> {
    if filter.limit < 1 || filter.limit > 1_000 {
        return Err(anyhow!("repair limit must be between 1 and 1000"));
    }
    if filter.event_ids.is_empty() && filter.exact_error.is_none() {
        return Err(anyhow!("repair requires event IDs or an exact error"));
    }
    if !filter.event_ids.is_empty() && filter.exact_error.is_some() {
        return Err(anyhow!(
            "explicit event IDs cannot be combined with BadJwt class mode"
        ));
    }
    Ok(())
}

/// Revalidate and make exactly the previewed legacy jobs claimable again.
pub fn revive_legacy_badjwt_jobs(
    conn: &mut PgConnection,
    filter: &LegacyBadJwtRepairFilter,
    previewed_event_ids: &[String],
) -> Result<usize> {
    validate_legacy_repair_filter(filter)?;
    if previewed_event_ids.is_empty() {
        return Ok(0);
    }
    let query = "UPDATE publish_jobs p
         SET completed_at = NULL, lease_owner = NULL,
             lease_expires_at = NOW(), writer_epoch = 2, updated_at = NOW()
         FROM account_links a
         WHERE a.nostr_pubkey = p.nostr_pubkey
           AND p.nostr_pubkey = $1
           AND a.provisioning_state = 'ready'
           AND a.crosspost_enabled = TRUE
           AND p.state = 'failed'
           AND p.completed_at IS NOT NULL
           AND p.lease_owner IS NULL
           AND (p.lease_expires_at IS NULL OR p.lease_expires_at <= NOW())
           AND (
             (cardinality($2::text[]) > 0 AND p.nostr_event_id = ANY($2))
             OR (
               $3::text IS NOT NULL
               AND (
                 p.error = $3
                 OR (
                   $3 = 'BadJwt: Signature tag didn''t verify'
                   AND POSITION('getServiceAuth failed (400)' IN p.error) > 0
                   AND POSITION('BadJwt: Signature tag didn''t verify' IN p.error) > 0
                 )
               )
             )
           )
           AND p.nostr_event_id = ANY($4)";
    let changed = sql_query(query)
        .bind::<Text, _>(&filter.nostr_pubkey)
        .bind::<Array<Text>, _>(&filter.event_ids)
        .bind::<Nullable<Text>, _>(filter.exact_error.as_deref())
        .bind::<Array<Text>, _>(previewed_event_ids)
        .execute(conn)?;
    Ok(changed)
}

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

/// Idempotently enqueue a publish job keyed by source Nostr event ID.
///
/// If a job already exists (including published/skipped terminal rows), that
/// existing row is returned unchanged.
pub fn enqueue_publish_job(conn: &mut PgConnection, job: &NewPublishJob) -> Result<PublishJob> {
    conn.transaction(|conn| {
        let inserted = diesel::insert_into(publish_jobs::table)
            .values(job)
            .on_conflict(publish_jobs::nostr_event_id)
            .do_nothing()
            .execute(conn)?;

        // The schema default remains epoch 1 so an old binary overlapping the
        // migration can only create quarantined legacy work.  Only this new
        // enqueue path promotes rows it inserted to the create-only epoch.
        if inserted == 1 {
            diesel::update(publish_jobs::table.find(job.nostr_event_id))
                .set(publish_jobs::writer_epoch.eq(2))
                .execute(conn)?;
        } else {
            // During a rolling overlap an old ingester may win the insert with
            // epoch 1. A completely pristine row proves no worker ever claimed
            // it and no remote side effect is possible, so adopting that row is
            // safe. Anything attempted remains quarantined for operator audit.
            diesel::update(
                publish_jobs::table
                    .filter(publish_jobs::nostr_event_id.eq(job.nostr_event_id))
                    .filter(publish_jobs::writer_epoch.eq(1))
                    .filter(publish_jobs::state.eq(PublishState::Pending.as_str()))
                    .filter(publish_jobs::attempt.eq(0))
                    .filter(publish_jobs::reserved_rkey.is_null())
                    .filter(publish_jobs::prepared_record.is_null())
                    .filter(publish_jobs::lease_owner.is_null())
                    .filter(publish_jobs::completed_at.is_null()),
            )
            .set(publish_jobs::writer_epoch.eq(2))
            .execute(conn)?;
        }

        get_publish_job(conn, job.nostr_event_id)?.ok_or_else(|| {
            anyhow!(
                "publish job missing after enqueue for {}",
                job.nostr_event_id
            )
        })
    })
}

/// Durably reserve the one AT record key used for every execution of a job.
///
/// The first candidate wins. Later calls return the stored key, including
/// concurrent retries after a worker loses its lease. An advisory lock and
/// same-account check turn a generator collision into an error before any
/// remote side effect without requiring a blocking startup index build.
pub fn reserve_publish_job_rkey(
    conn: &mut PgConnection,
    nostr_event_id: &str,
    candidate: &str,
) -> Result<String> {
    conn.transaction(|conn| {
        // Serialize the rare same-candidate path without building a blocking
        // index during startup migration. Rkeys are repository-scoped; the
        // Nostr pubkey identifies the linked target account here.
        sql_query(
            "SELECT TRUE AS value
             FROM pg_advisory_xact_lock(hashtextextended($1, 0))",
        )
        .bind::<Text, _>(candidate)
        .get_result::<BooleanRow>(conn)?;

        let collision = sql_query(
            "SELECT EXISTS (
                SELECT 1
                FROM publish_jobs AS other
                JOIN publish_jobs AS current
                  ON current.nostr_event_id = $1
                WHERE other.nostr_pubkey = current.nostr_pubkey
                  AND other.reserved_rkey = $2
                  AND other.nostr_event_id <> current.nostr_event_id
             ) AS value",
        )
        .bind::<Text, _>(nostr_event_id)
        .bind::<Text, _>(candidate)
        .get_result::<BooleanRow>(conn)?
        .value;
        if collision {
            return Err(anyhow!(
                "reserved rkey collision for linked account: {candidate}"
            ));
        }

        let row = sql_query(
            "UPDATE publish_jobs
             SET reserved_rkey = COALESCE(reserved_rkey, $2),
                 updated_at = CASE WHEN reserved_rkey IS NULL THEN NOW() ELSE updated_at END
             WHERE nostr_event_id = $1
             RETURNING reserved_rkey",
        )
        .bind::<Text, _>(nostr_event_id)
        .bind::<Text, _>(candidate)
        .get_result::<ReservedRkeyRow>(conn)
        .with_context(|| format!("failed to reserve publish rkey for {nostr_event_id}"))?;
        Ok(row.reserved_rkey)
    })
}

/// Persist the exact AT record that every attempt of this job must create.
///
/// The first prepared value wins.  Returning the stored value is essential:
/// concurrent workers may prepare different repo-scoped blob CIDs, but only
/// one canonical record is allowed to reach `createRecord`.
pub fn reserve_publish_job_prepared_record(
    conn: &mut PgConnection,
    nostr_event_id: &str,
    candidate: &serde_json::Value,
) -> Result<serde_json::Value> {
    let row = sql_query(
        "UPDATE publish_jobs
         SET prepared_record = COALESCE(prepared_record, $2),
             updated_at = CASE WHEN prepared_record IS NULL THEN NOW() ELSE updated_at END
         WHERE nostr_event_id = $1
         RETURNING prepared_record",
    )
    .bind::<Text, _>(nostr_event_id)
    .bind::<Jsonb, _>(candidate)
    .get_result::<PreparedRecordRow>(conn)
    .with_context(|| format!("failed to reserve prepared record for {nostr_event_id}"))?;
    Ok(row.prepared_record)
}

fn claim_next_job(
    conn: &mut PgConnection,
    source: PublishJobSource,
    lease_owner: &str,
    lease_expires_at: DateTime<Utc>,
) -> Result<Option<PublishJob>> {
    let order_by = match source {
        PublishJobSource::Live => "created_at ASC, nostr_event_id ASC",
        PublishJobSource::Backfill => "event_created_at ASC, created_at ASC, nostr_event_id ASC",
    };
    let query = format!(
        "WITH writer_fence AS MATERIALIZED (
            SELECT set_config('divine_sky.writer_epoch', '2', true)
         ), candidate AS (
            SELECT nostr_event_id
            FROM publish_jobs, writer_fence
            WHERE job_source = $1
              AND writer_epoch = 2
              AND (
                state = 'pending'
                OR (
                    state = 'failed'
                    AND completed_at IS NULL
                    AND (lease_expires_at IS NULL OR lease_expires_at <= NOW())
                )
                OR (state = 'in_progress' AND lease_expires_at IS NOT NULL AND lease_expires_at <= NOW())
              )
            ORDER BY {order_by}
            LIMIT 1
            FOR UPDATE SKIP LOCKED
         )
         UPDATE publish_jobs
         SET state = 'in_progress',
             attempt = attempt + 1,
             error = NULL,
             lease_owner = $2,
             lease_expires_at = $3,
             completed_at = NULL,
             updated_at = NOW()
         FROM candidate
         WHERE publish_jobs.nostr_event_id = candidate.nostr_event_id
         RETURNING publish_jobs.nostr_event_id"
    );

    let claimed = sql_query(query)
        .bind::<Text, _>(source.as_str())
        .bind::<Text, _>(lease_owner)
        .bind::<Timestamptz, _>(lease_expires_at)
        .get_result::<PublishJobIdRow>(conn)
        .optional()?;

    if let Some(claimed) = claimed {
        return get_publish_job(conn, &claimed.nostr_event_id);
    }

    Ok(None)
}

/// Claim the next live-lane publish job.
pub fn claim_next_live_job(
    conn: &mut PgConnection,
    lease_owner: &str,
    lease_expires_at: DateTime<Utc>,
) -> Result<Option<PublishJob>> {
    claim_next_job(conn, PublishJobSource::Live, lease_owner, lease_expires_at)
}

/// Claim the next backlog-lane publish job ordered oldest-first.
pub fn claim_next_backfill_job(
    conn: &mut PgConnection,
    lease_owner: &str,
    lease_expires_at: DateTime<Utc>,
) -> Result<Option<PublishJob>> {
    claim_next_job(
        conn,
        PublishJobSource::Backfill,
        lease_owner,
        lease_expires_at,
    )
}

/// Mark a publish job as completed/published.
pub fn mark_publish_job_completed(
    conn: &mut PgConnection,
    nostr_event_id: &str,
) -> Result<PublishJob> {
    mark_publish_job_completed_inner(conn, nostr_event_id, None)
}

/// Complete a job only if this worker still owns the active lease.
pub fn mark_publish_job_completed_for_owner(
    conn: &mut PgConnection,
    nostr_event_id: &str,
    lease_owner: &str,
) -> Result<PublishJob> {
    mark_publish_job_completed_inner(conn, nostr_event_id, Some(lease_owner))
}

fn mark_publish_job_completed_inner(
    conn: &mut PgConnection,
    nostr_event_id: &str,
    lease_owner: Option<&str>,
) -> Result<PublishJob> {
    let now = Utc::now();
    let result = if let Some(lease_owner) = lease_owner {
        diesel::update(
            publish_jobs::table
                .filter(publish_jobs::nostr_event_id.eq(nostr_event_id))
                .filter(publish_jobs::state.eq(PublishState::InProgress.as_str()))
                .filter(publish_jobs::lease_owner.eq(lease_owner)),
        )
        .set((
            publish_jobs::state.eq(PublishState::Published.as_str()),
            publish_jobs::error.eq(None::<String>),
            publish_jobs::lease_owner.eq(None::<String>),
            publish_jobs::lease_expires_at.eq(None::<DateTime<Utc>>),
            publish_jobs::completed_at.eq(Some(now)),
            publish_jobs::updated_at.eq(now),
        ))
        .get_result::<PublishJob>(conn)
    } else {
        diesel::update(publish_jobs::table.find(nostr_event_id))
            .set((
                publish_jobs::state.eq(PublishState::Published.as_str()),
                publish_jobs::error.eq(None::<String>),
                publish_jobs::lease_owner.eq(None::<String>),
                publish_jobs::lease_expires_at.eq(None::<DateTime<Utc>>),
                publish_jobs::completed_at.eq(Some(now)),
                publish_jobs::updated_at.eq(now),
            ))
            .get_result::<PublishJob>(conn)
    }
    .with_context(|| format!("publish job lease lost before completion: {nostr_event_id}"))?;
    Ok(result)
}

/// Extend an active lease without allowing a stale worker to reacquire it.
pub fn renew_publish_job_lease(
    conn: &mut PgConnection,
    nostr_event_id: &str,
    lease_owner: &str,
    lease_expires_at: DateTime<Utc>,
) -> Result<bool> {
    let changed = diesel::update(
        publish_jobs::table
            .filter(publish_jobs::nostr_event_id.eq(nostr_event_id))
            .filter(publish_jobs::state.eq(PublishState::InProgress.as_str()))
            .filter(publish_jobs::lease_owner.eq(lease_owner)),
    )
    .set((
        publish_jobs::lease_expires_at.eq(Some(lease_expires_at)),
        publish_jobs::updated_at.eq(Utc::now()),
    ))
    .execute(conn)?;
    Ok(changed == 1)
}

/// Maximum failed attempts before a publish job is terminally failed.
const MAX_PUBLISH_JOB_ATTEMPTS: i32 = 20;
/// Upper bound on the exponential retry backoff for failed publish jobs.
/// How long to park a LIVE job that hit the video service's per-DID daily upload
/// quota. Short enough that a fresh post publishes soon after the window reopens.
const UPLOAD_QUOTA_RETRY_SECS_LIVE: i64 = 60 * 60;

/// How long to park a BACKFILL job on the same quota. Deliberately far longer
/// than the live delay: a large catalog replay would otherwise consume the whole
/// daily allowance every time the window reopens and starve the user's fresh
/// posts. Backfill yields; live goes first.
const UPLOAD_QUOTA_RETRY_SECS_BACKFILL: i64 = 12 * 60 * 60;

/// The video service reports its per-DID daily cap as `daily_vid_limit_exceeded`
/// (inside an HTTP 401 body, confusingly). Treat it as a throttle, not a defect.
fn is_upload_quota_error(error_msg: &str) -> bool {
    error_msg.contains("daily_vid_limit_exceeded")
}

const MAX_PUBLISH_JOB_BACKOFF_SECS: i64 = 600;

/// Mark a publish job attempt as failed.
///
/// Retryable failures keep a backoff lease: `lease_expires_at` is pushed to
/// `now + min(2^attempt, 600)` seconds so the claim query skips the job until
/// the backoff elapses instead of hot-looping on a permanently failing job.
/// After `MAX_PUBLISH_JOB_ATTEMPTS` the job fails terminally: `completed_at`
/// is set and the claim query never returns it again.
pub fn mark_publish_job_failed(
    conn: &mut PgConnection,
    nostr_event_id: &str,
    error_msg: &str,
) -> Result<PublishJob> {
    mark_publish_job_failed_inner(conn, nostr_event_id, error_msg, None)
}

/// Fail a job only if this worker still owns the active lease.
pub fn mark_publish_job_failed_for_owner(
    conn: &mut PgConnection,
    nostr_event_id: &str,
    lease_owner: &str,
    error_msg: &str,
) -> Result<PublishJob> {
    mark_publish_job_failed_inner(conn, nostr_event_id, error_msg, Some(lease_owner))
}

fn mark_publish_job_failed_inner(
    conn: &mut PgConnection,
    nostr_event_id: &str,
    error_msg: &str,
    lease_owner: Option<&str>,
) -> Result<PublishJob> {
    let mut owned = publish_jobs::table
        .filter(publish_jobs::nostr_event_id.eq(nostr_event_id))
        .into_boxed();
    if let Some(lease_owner) = lease_owner {
        owned = owned
            .filter(publish_jobs::state.eq(PublishState::InProgress.as_str()))
            .filter(publish_jobs::lease_owner.eq(lease_owner));
    }
    let existing = owned
        .first::<PublishJob>(conn)
        .optional()?
        .ok_or_else(|| anyhow!("publish job missing for {nostr_event_id}"))?;
    let now = Utc::now();

    // A throttle is not a failure: Bluesky's video service caps uploads per DID
    // per day, and past the cap every upload is rejected. Counting those toward
    // the attempt budget would terminally fail a large catalog long before the
    // quota resets. Park the job until the next window WITHOUT spending an
    // attempt, so backfills drain themselves across days.
    if is_upload_quota_error(error_msg) {
        // Backfill waits far longer than live so a catalog replay cannot eat the
        // daily allowance and starve the user's fresh posts.
        let park_secs = if existing.job_source == PublishJobSource::Backfill.as_str() {
            UPLOAD_QUOTA_RETRY_SECS_BACKFILL
        } else {
            UPLOAD_QUOTA_RETRY_SECS_LIVE
        };
        let result = diesel::update(
            publish_jobs::table
                .filter(publish_jobs::nostr_event_id.eq(nostr_event_id))
                .filter(
                    publish_jobs::lease_owner.is_not_distinct_from(existing.lease_owner.clone()),
                ),
        )
        .set((
            publish_jobs::state.eq(PublishState::Failed.as_str()),
            publish_jobs::error.eq(Some(error_msg.to_string())),
            publish_jobs::lease_expires_at.eq(Some(now + chrono::Duration::seconds(park_secs))),
            publish_jobs::completed_at.eq(None::<DateTime<Utc>>),
            publish_jobs::updated_at.eq(now),
        ))
        .get_result::<PublishJob>(conn)?;
        return Ok(result);
    }

    let attempt = existing.attempt.saturating_add(1);

    if attempt >= MAX_PUBLISH_JOB_ATTEMPTS {
        let result = diesel::update(
            publish_jobs::table
                .filter(publish_jobs::nostr_event_id.eq(nostr_event_id))
                .filter(
                    publish_jobs::lease_owner.is_not_distinct_from(existing.lease_owner.clone()),
                ),
        )
        .set((
            publish_jobs::state.eq(PublishState::Failed.as_str()),
            publish_jobs::attempt.eq(attempt),
            publish_jobs::error.eq(Some(error_msg.to_string())),
            publish_jobs::lease_owner.eq(None::<String>),
            publish_jobs::lease_expires_at.eq(None::<DateTime<Utc>>),
            publish_jobs::completed_at.eq(Some(now)),
            publish_jobs::updated_at.eq(now),
        ))
        .get_result::<PublishJob>(conn)?;
        return Ok(result);
    }

    let backoff_secs = 2i64
        .checked_pow(attempt.clamp(0, 30) as u32)
        .unwrap_or(MAX_PUBLISH_JOB_BACKOFF_SECS)
        .min(MAX_PUBLISH_JOB_BACKOFF_SECS);
    let result = diesel::update(
        publish_jobs::table
            .filter(publish_jobs::nostr_event_id.eq(nostr_event_id))
            .filter(publish_jobs::lease_owner.is_not_distinct_from(existing.lease_owner.clone())),
    )
    .set((
        publish_jobs::state.eq(PublishState::Failed.as_str()),
        publish_jobs::attempt.eq(attempt),
        publish_jobs::error.eq(Some(error_msg.to_string())),
        publish_jobs::lease_expires_at.eq(Some(now + chrono::Duration::seconds(backoff_secs))),
        publish_jobs::completed_at.eq(None::<DateTime<Utc>>),
        publish_jobs::updated_at.eq(now),
    ))
    .get_result::<PublishJob>(conn)?;
    Ok(result)
}

fn mark_publish_job_skipped(
    conn: &mut PgConnection,
    nostr_event_id: &str,
    error_msg: Option<&str>,
) -> Result<PublishJob> {
    let now = Utc::now();
    let result = diesel::update(publish_jobs::table.find(nostr_event_id))
        .set((
            publish_jobs::state.eq(PublishState::Skipped.as_str()),
            publish_jobs::error.eq(error_msg.map(str::to_string)),
            publish_jobs::lease_owner.eq(None::<String>),
            publish_jobs::lease_expires_at.eq(None::<DateTime<Utc>>),
            publish_jobs::completed_at.eq(Some(now)),
            publish_jobs::updated_at.eq(now),
        ))
        .get_result::<PublishJob>(conn)?;
    Ok(result)
}

/// Cancel a publish job due to a delete signal.
///
/// If the row does not yet exist, inserts a tombstone row in `skipped` state.
/// Existing `published` and `skipped` rows are left untouched.
pub fn cancel_publish_job(
    conn: &mut PgConnection,
    tombstone_job: &NewPublishJob,
    error_msg: Option<&str>,
) -> Result<PublishJob> {
    let nostr_event_id = tombstone_job.nostr_event_id;
    if let Some(existing) = get_publish_job(conn, nostr_event_id)? {
        if existing.state == PublishState::Published.as_str()
            || existing.state == PublishState::Skipped.as_str()
        {
            return Ok(existing);
        }
        return mark_publish_job_skipped(conn, nostr_event_id, error_msg);
    }

    let tombstone = NewPublishJob {
        nostr_event_id,
        nostr_pubkey: tombstone_job.nostr_pubkey,
        event_created_at: tombstone_job.event_created_at,
        event_payload: tombstone_job.event_payload.clone(),
        job_source: tombstone_job.job_source,
        state: PublishState::Skipped.as_str(),
    };
    diesel::insert_into(publish_jobs::table)
        .values(&tombstone)
        .on_conflict(publish_jobs::nostr_event_id)
        .do_nothing()
        .execute(conn)?;

    let existing = get_publish_job(conn, nostr_event_id)?
        .ok_or_else(|| anyhow!("publish job missing after cancel for {nostr_event_id}"))?;
    if existing.state == PublishState::Skipped.as_str() && existing.completed_at.is_some() {
        return Ok(existing);
    }

    mark_publish_job_skipped(conn, nostr_event_id, error_msg)
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

/// Insert a new publish job (legacy helper).
pub fn insert_publish_job(conn: &mut PgConnection, job: &NewPublishJob) -> Result<PublishJob> {
    enqueue_publish_job(conn, job)
}

/// Update a publish job's state and attempt count (legacy helper).
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

// ---------------------------------------------------------------------------
// appview read-model queries
// ---------------------------------------------------------------------------

pub fn upsert_appview_repo(conn: &mut PgConnection, repo: &NewAppviewRepo) -> Result<AppviewRepo> {
    let result = diesel::insert_into(appview_repos::table)
        .values(repo)
        .on_conflict(appview_repos::did)
        .do_update()
        .set((
            appview_repos::handle.eq(repo.handle),
            appview_repos::head.eq(repo.head),
            appview_repos::rev.eq(repo.rev),
            appview_repos::active.eq(repo.active),
            appview_repos::last_backfilled_at.eq(repo.last_backfilled_at),
            appview_repos::last_seen_seq.eq(repo.last_seen_seq),
            appview_repos::updated_at.eq(diesel::dsl::now),
        ))
        .get_result::<AppviewRepo>(conn)?;
    Ok(result)
}

pub fn upsert_appview_profile(
    conn: &mut PgConnection,
    profile: &NewAppviewProfile,
) -> Result<AppviewProfile> {
    let result = diesel::insert_into(appview_profiles::table)
        .values(profile)
        .on_conflict(appview_profiles::did)
        .do_update()
        .set((
            appview_profiles::handle.eq(profile.handle),
            appview_profiles::display_name.eq(profile.display_name),
            appview_profiles::description.eq(profile.description),
            appview_profiles::website.eq(profile.website),
            appview_profiles::avatar_cid.eq(profile.avatar_cid),
            appview_profiles::banner_cid.eq(profile.banner_cid),
            appview_profiles::created_at.eq(profile.created_at),
            appview_profiles::raw_json.eq(profile.raw_json),
            appview_profiles::indexed_at.eq(profile.indexed_at),
            appview_profiles::updated_at.eq(diesel::dsl::now),
        ))
        .get_result::<AppviewProfile>(conn)?;
    Ok(result)
}

pub fn upsert_appview_post(conn: &mut PgConnection, post: &NewAppviewPost) -> Result<AppviewPost> {
    let result = diesel::insert_into(appview_posts::table)
        .values(post)
        .on_conflict(appview_posts::uri)
        .do_update()
        .set((
            appview_posts::did.eq(post.did),
            appview_posts::rkey.eq(post.rkey),
            appview_posts::record_cid.eq(post.record_cid),
            appview_posts::created_at.eq(post.created_at),
            appview_posts::text.eq(post.text),
            appview_posts::langs_json.eq(post.langs_json),
            appview_posts::embed_blob_cid.eq(post.embed_blob_cid),
            appview_posts::embed_alt.eq(post.embed_alt),
            appview_posts::aspect_ratio_width.eq(post.aspect_ratio_width),
            appview_posts::aspect_ratio_height.eq(post.aspect_ratio_height),
            appview_posts::raw_json.eq(post.raw_json),
            appview_posts::search_text.eq(post.search_text),
            appview_posts::indexed_at.eq(post.indexed_at),
            appview_posts::deleted_at.eq(post.deleted_at),
        ))
        .get_result::<AppviewPost>(conn)?;
    Ok(result)
}

pub fn upsert_appview_media_view(
    conn: &mut PgConnection,
    view: &NewAppviewMediaView,
) -> Result<AppviewMediaView> {
    let result = diesel::insert_into(appview_media_views::table)
        .values(view)
        .on_conflict((appview_media_views::did, appview_media_views::blob_cid))
        .do_update()
        .set((
            appview_media_views::playlist_url.eq(view.playlist_url),
            appview_media_views::thumbnail_url.eq(view.thumbnail_url),
            appview_media_views::mime_type.eq(view.mime_type),
            appview_media_views::bytes.eq(view.bytes),
            appview_media_views::ready.eq(view.ready),
            appview_media_views::last_derived_at.eq(view.last_derived_at),
            appview_media_views::updated_at.eq(diesel::dsl::now),
        ))
        .get_result::<AppviewMediaView>(conn)?;
    Ok(result)
}

pub fn upsert_appview_service_state(
    conn: &mut PgConnection,
    state: &NewAppviewServiceState,
) -> Result<AppviewServiceState> {
    let result = diesel::insert_into(appview_service_state::table)
        .values(state)
        .on_conflict(appview_service_state::state_key)
        .do_update()
        .set((
            appview_service_state::state_value.eq(state.state_value),
            appview_service_state::updated_at.eq(diesel::dsl::now),
        ))
        .get_result::<AppviewServiceState>(conn)?;
    Ok(result)
}

pub fn get_appview_service_state(
    conn: &mut PgConnection,
    key: &str,
) -> Result<Option<AppviewServiceState>> {
    let result = appview_service_state::table
        .find(key)
        .first::<AppviewServiceState>(conn)
        .optional()?;
    Ok(result)
}

pub fn get_appview_profile_by_actor(
    conn: &mut PgConnection,
    actor: &str,
) -> Result<Option<AppviewProfile>> {
    let result = appview_profiles::table
        .filter(
            appview_profiles::did
                .eq(actor)
                .or(appview_profiles::handle.eq(Some(actor))),
        )
        .first::<AppviewProfile>(conn)
        .optional()?;
    Ok(result)
}

pub fn get_appview_media_view(
    conn: &mut PgConnection,
    did: &str,
    blob_cid: &str,
) -> Result<Option<AppviewMediaView>> {
    let result = appview_media_views::table
        .find((did, blob_cid))
        .first::<AppviewMediaView>(conn)
        .optional()?;
    Ok(result)
}

pub fn list_author_feed(
    conn: &mut PgConnection,
    actor: &str,
    limit: i64,
    cursor: Option<DateTime<Utc>>,
) -> Result<Vec<AppviewPost>> {
    let Some(profile) = get_appview_profile_by_actor(conn, actor)? else {
        return Ok(vec![]);
    };

    let mut query = appview_posts::table
        .filter(appview_posts::did.eq(profile.did))
        .filter(appview_posts::deleted_at.is_null())
        .into_boxed();

    if let Some(cursor) = cursor {
        query = query.filter(appview_posts::created_at.lt(cursor));
    }

    let results = query
        .order((appview_posts::created_at.desc(), appview_posts::uri.desc()))
        .limit(limit)
        .load::<AppviewPost>(conn)?;
    Ok(results)
}

pub fn list_latest_appview_posts(conn: &mut PgConnection, limit: i64) -> Result<Vec<AppviewPost>> {
    let results = appview_posts::table
        .filter(appview_posts::deleted_at.is_null())
        .order((appview_posts::created_at.desc(), appview_posts::uri.desc()))
        .limit(limit)
        .load::<AppviewPost>(conn)?;
    Ok(results)
}

pub fn list_trending_appview_posts(
    conn: &mut PgConnection,
    limit: i64,
) -> Result<Vec<AppviewPost>> {
    list_latest_appview_posts(conn, limit)
}

pub fn search_appview_posts(
    conn: &mut PgConnection,
    query_text: &str,
    limit: i64,
) -> Result<Vec<AppviewPost>> {
    let pattern = format!("%{}%", query_text);
    let results = appview_posts::table
        .filter(appview_posts::deleted_at.is_null())
        .filter(appview_posts::search_text.ilike(pattern))
        .order((appview_posts::created_at.desc(), appview_posts::uri.desc()))
        .limit(limit)
        .load::<AppviewPost>(conn)?;
    Ok(results)
}

pub fn load_posts_by_uris(conn: &mut PgConnection, uris: &[String]) -> Result<Vec<AppviewPost>> {
    if uris.is_empty() {
        return Ok(vec![]);
    }

    let results = appview_posts::table
        .filter(appview_posts::uri.eq_any(uris))
        .filter(appview_posts::deleted_at.is_null())
        .order(appview_posts::created_at.desc())
        .load::<AppviewPost>(conn)?;
    Ok(results)
}

pub fn load_post_with_media_view(
    conn: &mut PgConnection,
    uri: &str,
) -> Result<Option<AppviewPostWithMediaViewRow>> {
    let result = sql_query(
        "SELECT
            p.uri,
            p.did,
            p.rkey,
            p.record_cid,
            p.created_at,
            p.text,
            p.langs_json,
            p.embed_blob_cid,
            p.embed_alt,
            p.aspect_ratio_width,
            p.aspect_ratio_height,
            p.raw_json,
            p.search_text,
            p.indexed_at,
            p.deleted_at,
            mv.playlist_url,
            mv.thumbnail_url,
            mv.mime_type AS media_mime_type,
            mv.bytes AS media_bytes,
            mv.ready AS media_ready
        FROM appview_posts p
        LEFT JOIN appview_media_views mv
          ON mv.did = p.did
         AND mv.blob_cid = p.embed_blob_cid
        WHERE p.uri = $1
          AND p.deleted_at IS NULL
        LIMIT 1",
    )
    .bind::<Text, _>(uri)
    .get_result::<AppviewPostWithMediaViewRow>(conn)
    .optional()?;
    Ok(result)
}
