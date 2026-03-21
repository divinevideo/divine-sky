//! Named queries for the Divine Bridge database.
//!
//! All query functions live here and are re-exported from the crate root.

use anyhow::Result;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Nullable, Text};
use diesel::PgConnection;
use diesel::PgTextExpressionMethods;

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

/// Look up lifecycle-aware account-link state by handle.
pub fn get_account_link_lifecycle_by_handle(
    conn: &mut PgConnection,
    handle: &str,
) -> Result<Option<AccountLinkLifecycleRow>> {
    let result = sql_query(
        "SELECT nostr_pubkey, did, handle, crosspost_enabled, signing_key_id, \
         plc_rotation_key_ref, provisioning_state, provisioning_error, disabled_at, \
         created_at, updated_at \
         FROM account_links WHERE handle = $1",
    )
    .bind::<Text, _>(handle)
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
                   plc_rotation_key_ref, provisioning_state, provisioning_error, disabled_at,
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
         SET crosspost_enabled = FALSE,
             provisioning_state = 'disabled',
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
