use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Duration, Utc};
use diesel::connection::SimpleConnection;
use diesel::Connection;
use diesel::PgConnection;
use diesel::RunQueryDsl;
use divine_bridge_db::models::NewPublishJob;
use divine_bridge_db::{
    cancel_publish_job, claim_next_backfill_job, claim_next_live_job, enqueue_publish_job,
    get_publish_job, list_accounts_requiring_backfill, mark_account_backfill_completed,
    mark_account_backfill_failed, mark_account_backfill_started, mark_publish_job_completed,
    mark_publish_job_completed_for_owner, mark_publish_job_failed, renew_publish_job_lease,
    reserve_publish_job_prepared_record, reserve_publish_job_rkey,
};
use divine_bridge_types::PublishJobSource;
use serde_json::json;

fn test_database_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://divine:divine_dev@[::1]:5432/divine_bridge".to_string())
}

fn execute_batch(conn: &mut PgConnection, sql: &str) {
    conn.batch_execute(sql).unwrap();
}

fn reset_database(database_url: &str) {
    let mut conn =
        PgConnection::establish(database_url).expect("test database should be reachable");
    execute_batch(
        &mut conn,
        include_str!("../../../migrations/001_bridge_tables/down.sql"),
    );
    execute_batch(
        &mut conn,
        include_str!("../../../migrations/001_bridge_tables/up.sql"),
    );
    execute_batch(
        &mut conn,
        include_str!("../../../migrations/004_publish_job_scheduler/up.sql"),
    );
    execute_batch(
        &mut conn,
        include_str!("../../../migrations/008_publish_job_reserved_rkey/up.sql"),
    );
}

// Tests in this binary share the database, so they serialize on a
// process-wide marker lock (same pattern as runtime_resilience.rs).
fn test_db_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// Test fixture builder mirrors the account_links columns; the arg count is
// inherent to the schema, not a design smell.
#[allow(clippy::too_many_arguments)]
fn seed_account(
    conn: &mut PgConnection,
    nostr_pubkey: &str,
    did: &str,
    handle: &str,
    crosspost_enabled: bool,
    provisioning_state: &str,
    publish_backfill_state: &str,
    created_at: DateTime<Utc>,
) {
    diesel::sql_query(format!(
        "INSERT INTO account_links (
            nostr_pubkey, did, handle, crosspost_enabled, signing_key_id,
            plc_rotation_key_ref, provisioning_state, provisioning_error,
            publish_backfill_state, publish_backfill_started_at,
            publish_backfill_completed_at, publish_backfill_error,
            disabled_at, created_at, updated_at
         ) VALUES (
            '{nostr_pubkey}', '{did}', '{handle}', {crosspost_enabled},
            'signing-{nostr_pubkey}', 'rotation-{nostr_pubkey}', '{provisioning_state}', NULL,
            '{publish_backfill_state}', NULL, NULL, NULL, NULL,
            '{created_at}', '{created_at}'
         )"
    ))
    .execute(conn)
    .expect("account link should insert");
}

#[test]
fn reserve_publish_job_rkey_is_first_writer_wins() {
    let _guard = test_db_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let database_url = test_database_url();
    reset_database(&database_url);
    let mut conn =
        PgConnection::establish(&database_url).expect("test database should be reachable");
    let created_at = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    for event_id in ["evt-reserve-one", "evt-reserve-two"] {
        enqueue_publish_job(
            &mut conn,
            &NewPublishJob {
                nostr_event_id: event_id,
                nostr_pubkey: "npub1alice",
                event_created_at: created_at,
                event_payload: json!({"id": event_id}),
                job_source: PublishJobSource::Live.as_str(),
                state: "pending",
            },
        )
        .expect("job should enqueue");
    }

    let first = reserve_publish_job_rkey(&mut conn, "evt-reserve-one", "3abcfirsttid")
        .expect("first reservation should succeed");
    let repeated = reserve_publish_job_rkey(&mut conn, "evt-reserve-one", "3abcothertid")
        .expect("later reservation should return the stored value");
    assert_eq!(first, "3abcfirsttid");
    assert_eq!(repeated, first);

    let collision = reserve_publish_job_rkey(&mut conn, "evt-reserve-two", "3abcfirsttid");
    assert!(
        collision.is_err(),
        "one linked account must not reserve the same rkey twice"
    );

    let first_record = json!({"text": "first", "embed": {"video": {"ref": {"$link": "cid-a"}}}});
    let second_record = json!({"text": "second", "embed": {"video": {"ref": {"$link": "cid-b"}}}});
    let stored = reserve_publish_job_prepared_record(&mut conn, "evt-reserve-one", &first_record)
        .expect("first prepared record should persist");
    let repeated =
        reserve_publish_job_prepared_record(&mut conn, "evt-reserve-one", &second_record)
            .expect("later preparation should return the stored record");
    assert_eq!(stored, first_record);
    assert_eq!(repeated, first_record, "the first prepared record must win");
}

#[test]
fn writer_epoch_quarantines_legacy_jobs_and_fences_legacy_claims() {
    let _guard = test_db_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let database_url = test_database_url();
    reset_database(&database_url);
    let mut conn =
        PgConnection::establish(&database_url).expect("test database should be reachable");

    diesel::sql_query(
        "INSERT INTO publish_jobs (
            nostr_event_id, nostr_pubkey, event_created_at, event_payload,
            job_source, state
         ) VALUES ('evt-legacy', 'npub1legacy', NOW(), '{}', 'live', 'pending')",
    )
    .execute(&mut conn)
    .expect("legacy insert should use epoch 1 default");

    let new_worker = claim_next_live_job(
        &mut conn,
        "epoch-2-worker",
        Utc::now() + Duration::minutes(5),
    )
    .expect("new worker claim should not error");
    assert!(new_worker.is_none(), "epoch-1 work must remain quarantined");

    let legacy_claim = diesel::sql_query(
        "UPDATE publish_jobs
         SET state = 'in_progress', lease_owner = 'legacy-worker'
         WHERE nostr_event_id = 'evt-legacy'",
    )
    .execute(&mut conn);
    assert!(
        legacy_claim.is_err(),
        "a writer without the epoch marker must be rejected by the database"
    );

    enqueue_publish_job(
        &mut conn,
        &NewPublishJob {
            nostr_event_id: "evt-legacy",
            nostr_pubkey: "npub1legacy",
            event_created_at: Utc::now(),
            event_payload: json!({"id": "evt-legacy"}),
            job_source: PublishJobSource::Live.as_str(),
            state: "pending",
        },
    )
    .expect("new ingester should adopt a pristine legacy insert");
    let adopted = claim_next_live_job(
        &mut conn,
        "epoch-2-worker",
        Utc::now() + Duration::minutes(5),
    )
    .expect("adopted claim should not error")
    .expect("pristine legacy insert should become claimable");
    assert_eq!(adopted.writer_epoch, 2);
}

#[test]
fn lease_renewal_and_finalization_require_the_current_owner() {
    let _guard = test_db_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let database_url = test_database_url();
    reset_database(&database_url);
    let mut conn =
        PgConnection::establish(&database_url).expect("test database should be reachable");
    let created_at = Utc::now();
    enqueue_publish_job(
        &mut conn,
        &NewPublishJob {
            nostr_event_id: "evt-owned-lease",
            nostr_pubkey: "npub1alice",
            event_created_at: created_at,
            event_payload: json!({"id": "evt-owned-lease"}),
            job_source: PublishJobSource::Live.as_str(),
            state: "pending",
        },
    )
    .expect("job should enqueue");
    claim_next_live_job(&mut conn, "worker-a", Utc::now() + Duration::minutes(5))
        .expect("claim should work")
        .expect("job should be claimable");

    assert!(renew_publish_job_lease(
        &mut conn,
        "evt-owned-lease",
        "worker-a",
        Utc::now() + Duration::minutes(10),
    )
    .expect("owner renewal should work"));
    assert!(!renew_publish_job_lease(
        &mut conn,
        "evt-owned-lease",
        "worker-b",
        Utc::now() + Duration::minutes(10),
    )
    .expect("non-owner renewal should be a clean miss"));
    assert!(
        mark_publish_job_completed_for_owner(&mut conn, "evt-owned-lease", "worker-b").is_err()
    );
    let completed = mark_publish_job_completed_for_owner(&mut conn, "evt-owned-lease", "worker-a")
        .expect("current owner should complete");
    assert_eq!(completed.state, "published");
}

#[test]
fn queue_and_backfill_queries_follow_scheduler_contract() {
    let _guard = test_db_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let database_url = test_database_url();
    reset_database(&database_url);
    let mut conn =
        PgConnection::establish(&database_url).expect("test database should be reachable");

    // enqueue idempotency + payload preservation
    let created_at = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let original_payload = json!({"id": "evt-1", "content": "first"});
    let first = enqueue_publish_job(
        &mut conn,
        &NewPublishJob {
            nostr_event_id: "evt-1",
            nostr_pubkey: "npub1alice",
            event_created_at: created_at,
            event_payload: original_payload.clone(),
            job_source: PublishJobSource::Live.as_str(),
            state: "pending",
        },
    )
    .expect("first enqueue should insert");
    let second = enqueue_publish_job(
        &mut conn,
        &NewPublishJob {
            nostr_event_id: "evt-1",
            nostr_pubkey: "npub1alice",
            event_created_at: created_at,
            event_payload: json!({"id": "evt-1", "content": "second"}),
            job_source: PublishJobSource::Backfill.as_str(),
            state: "pending",
        },
    )
    .expect("second enqueue should be idempotent");
    assert_eq!(first.nostr_event_id, "evt-1");
    assert_eq!(second.event_payload, original_payload);
    assert_eq!(second.job_source, PublishJobSource::Live.as_str());
    mark_publish_job_completed(&mut conn, "evt-1").expect("seed idempotency job should complete");

    // tombstone upsert must block later enqueue attempts
    let delete_created_at = DateTime::parse_from_rfc3339("2024-01-01T01:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let tombstone = cancel_publish_job(
        &mut conn,
        &NewPublishJob {
            nostr_event_id: "evt-delete-first",
            nostr_pubkey: "npub1alice",
            event_created_at: delete_created_at,
            event_payload: json!({
                "id": "evt-delete-first",
                "kind": 5,
                "tags": [["e", "evt-delete-first"]]
            }),
            job_source: PublishJobSource::Backfill.as_str(),
            state: "pending",
        },
        Some("delete-before-publish"),
    )
    .expect("cancel should create tombstone");
    assert_eq!(tombstone.state, "skipped");
    let enqueue_attempt = enqueue_publish_job(
        &mut conn,
        &NewPublishJob {
            nostr_event_id: "evt-delete-first",
            nostr_pubkey: "npub1alice",
            event_created_at: delete_created_at,
            event_payload: json!({"id": "evt-delete-first", "content": "ignored"}),
            job_source: PublishJobSource::Live.as_str(),
            state: "pending",
        },
    )
    .expect("enqueue should return existing tombstone");
    assert_eq!(enqueue_attempt.state, "skipped");

    // live lane claims only live jobs
    let old = DateTime::parse_from_rfc3339("2024-01-01T02:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let newer = DateTime::parse_from_rfc3339("2024-01-01T03:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    enqueue_publish_job(
        &mut conn,
        &NewPublishJob {
            nostr_event_id: "evt-live",
            nostr_pubkey: "npub1live",
            event_created_at: newer,
            event_payload: json!({"id": "evt-live"}),
            job_source: PublishJobSource::Live.as_str(),
            state: "pending",
        },
    )
    .expect("live job should enqueue");
    enqueue_publish_job(
        &mut conn,
        &NewPublishJob {
            nostr_event_id: "evt-backfill-old",
            nostr_pubkey: "npub1backfill",
            event_created_at: old,
            event_payload: json!({"id": "evt-backfill-old"}),
            job_source: PublishJobSource::Backfill.as_str(),
            state: "pending",
        },
    )
    .expect("old backfill should enqueue");
    enqueue_publish_job(
        &mut conn,
        &NewPublishJob {
            nostr_event_id: "evt-backfill-new",
            nostr_pubkey: "npub1backfill",
            event_created_at: newer,
            event_payload: json!({"id": "evt-backfill-new"}),
            job_source: PublishJobSource::Backfill.as_str(),
            state: "pending",
        },
    )
    .expect("new backfill should enqueue");

    let lease_expiry = Utc::now() + Duration::minutes(5);
    let live_claim = claim_next_live_job(&mut conn, "live-worker", lease_expiry)
        .expect("live claim should work")
        .expect("live claim should return a job");
    assert_eq!(live_claim.nostr_event_id, "evt-live");
    assert_eq!(live_claim.job_source, PublishJobSource::Live.as_str());
    let live_second = claim_next_live_job(&mut conn, "live-worker", lease_expiry)
        .expect("live claim should work");
    assert!(live_second.is_none(), "second live claim should be empty");

    // backfill lane claims only backfill jobs oldest first
    let backfill_first = claim_next_backfill_job(&mut conn, "backfill-worker", lease_expiry)
        .expect("backfill claim should work")
        .expect("backfill claim should return old job first");
    let backfill_second = claim_next_backfill_job(&mut conn, "backfill-worker", lease_expiry)
        .expect("backfill claim should work")
        .expect("backfill claim should return second job");
    assert_eq!(backfill_first.nostr_event_id, "evt-backfill-old");
    assert_eq!(backfill_second.nostr_event_id, "evt-backfill-new");

    // expired leases must be reclaimable
    let lease_created_at = DateTime::parse_from_rfc3339("2024-01-01T04:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    enqueue_publish_job(
        &mut conn,
        &NewPublishJob {
            nostr_event_id: "evt-expired-lease",
            nostr_pubkey: "npub1backfill",
            event_created_at: lease_created_at,
            event_payload: json!({"id": "evt-expired-lease"}),
            job_source: PublishJobSource::Backfill.as_str(),
            state: "pending",
        },
    )
    .expect("job should enqueue");
    let claimed = claim_next_backfill_job(&mut conn, "worker-a", lease_expiry)
        .expect("first claim should work")
        .expect("should claim job");
    assert_eq!(claimed.nostr_event_id, "evt-expired-lease");
    diesel::sql_query(
        "UPDATE publish_jobs
         SET lease_expires_at = NOW() - INTERVAL '1 second'
         WHERE nostr_event_id = 'evt-expired-lease'",
    )
    .execute(&mut conn)
    .expect("lease should be forced expired");
    let reclaimed = claim_next_backfill_job(&mut conn, "worker-b", lease_expiry)
        .expect("reclaim should work")
        .expect("expired lease should be reclaimable");
    assert_eq!(reclaimed.nostr_event_id, "evt-expired-lease");
    assert_eq!(reclaimed.lease_owner.as_deref(), Some("worker-b"));

    // backlog account ordering
    let t1 = DateTime::parse_from_rfc3339("2024-01-01T10:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let t2 = DateTime::parse_from_rfc3339("2024-01-01T11:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let t3 = DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let t4 = DateTime::parse_from_rfc3339("2024-01-01T13:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    seed_account(
        &mut conn,
        "npub1alice-backfill",
        "did:plc:alice-backfill",
        "alice-backfill.divine.video",
        true,
        "ready",
        "not_started",
        t2,
    );
    seed_account(
        &mut conn,
        "npub1bob-backfill",
        "did:plc:bob-backfill",
        "bob-backfill.divine.video",
        true,
        "ready",
        "failed",
        t3,
    );
    seed_account(
        &mut conn,
        "npub1skip-disabled",
        "did:plc:skip-disabled",
        "skip-disabled.divine.video",
        false,
        "ready",
        "not_started",
        t1,
    );
    seed_account(
        &mut conn,
        "npub1skip-pending",
        "did:plc:skip-pending",
        "skip-pending.divine.video",
        true,
        "pending",
        "not_started",
        t4,
    );
    let backlog = list_accounts_requiring_backfill(&mut conn, 10).expect("list should succeed");
    let pubkeys: Vec<String> = backlog.into_iter().map(|row| row.nostr_pubkey).collect();
    assert_eq!(
        pubkeys,
        vec![
            "npub1alice-backfill".to_string(),
            "npub1bob-backfill".to_string()
        ]
    );

    let started = mark_account_backfill_started(&mut conn, "npub1alice-backfill")
        .expect("start marker should work");
    assert_eq!(started.publish_backfill_state, "in_progress");
    assert!(started.publish_backfill_started_at.is_some());
    let failed = mark_account_backfill_failed(&mut conn, "npub1alice-backfill", "relay timeout")
        .expect("failed marker should work");
    assert_eq!(failed.publish_backfill_state, "failed");
    assert_eq!(
        failed.publish_backfill_error.as_deref(),
        Some("relay timeout")
    );
    let completed = mark_account_backfill_completed(&mut conn, "npub1alice-backfill")
        .expect("complete marker should work");
    assert_eq!(completed.publish_backfill_state, "completed");
    assert!(completed.publish_backfill_completed_at.is_some());

    // publish job state markers + get surface
    let marker_created_at = DateTime::parse_from_rfc3339("2024-01-01T14:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    enqueue_publish_job(
        &mut conn,
        &NewPublishJob {
            nostr_event_id: "evt-markers",
            nostr_pubkey: "npub1alice",
            event_created_at: marker_created_at,
            event_payload: json!({"id": "evt-markers"}),
            job_source: PublishJobSource::Live.as_str(),
            state: "pending",
        },
    )
    .expect("job should enqueue");
    let failed = mark_publish_job_failed(&mut conn, "evt-markers", "pds timeout")
        .expect("failed marker should work");
    assert_eq!(failed.state, "failed");
    assert_eq!(failed.error.as_deref(), Some("pds timeout"));
    assert_eq!(failed.attempt, 1);
    let completed =
        mark_publish_job_completed(&mut conn, "evt-markers").expect("complete marker should work");
    assert_eq!(completed.state, "published");
    assert!(completed.completed_at.is_some());
    assert!(completed.lease_owner.is_none());
    let loaded = get_publish_job(&mut conn, "evt-markers")
        .expect("get should succeed")
        .expect("job should exist");
    assert_eq!(loaded.state, "published");
}

#[test]
fn upload_quota_errors_defer_without_spending_attempts() {
    // Bluesky's video service caps uploads per DID per day. A catalog backfill
    // larger than the cap must park and resume next window, not burn its retry
    // budget and terminally fail.
    let _guard = test_db_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let database_url = test_database_url();
    reset_database(&database_url);
    let mut conn =
        PgConnection::establish(&database_url).expect("test database should be reachable");

    let created_at = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    enqueue_publish_job(
        &mut conn,
        &NewPublishJob {
            nostr_event_id: "evt-quota",
            nostr_pubkey: "npub1alice",
            event_created_at: created_at,
            event_payload: json!({"id": "evt-quota"}),
            job_source: PublishJobSource::Live.as_str(),
            state: "pending",
        },
    )
    .expect("job should enqueue");

    let claimed = claim_next_live_job(&mut conn, "worker-a", Utc::now() + Duration::minutes(5))
        .expect("claim should work")
        .expect("pending job should be claimable");
    let attempt_after_claim = claimed.attempt;

    let quota_error = "failed to upload to video service: uploadVideo failed (401): \
{\"jobStatus\":{\"error\":\"daily_vid_limit_exceeded\"}}";
    let before = Utc::now();
    let deferred = mark_publish_job_failed(&mut conn, "evt-quota", quota_error)
        .expect("quota failure should be recorded");

    // Parked, not counted: the attempt budget is untouched...
    assert_eq!(
        deferred.attempt, attempt_after_claim,
        "a quota throttle must not spend an attempt"
    );
    // ...the job stays reclaimable (never terminal)...
    assert!(deferred.completed_at.is_none());
    // ...and a LIVE job is held off for about an hour: long enough to stop
    // hammering the quota, short enough to publish soon after it resets.
    let lease = deferred
        .lease_expires_at
        .expect("quota deferral must set a lease");
    assert!(
        lease > before + Duration::minutes(30) && lease < before + Duration::hours(2),
        "a live job should park for ~1h, got {lease}"
    );

    // Until the lease expires it must not be claimed again.
    assert!(
        claim_next_live_job(&mut conn, "worker-b", Utc::now() + Duration::minutes(5))
            .expect("claim should work")
            .is_none(),
        "a quota-deferred job must not be reclaimed before its window"
    );
}

#[test]
fn failed_jobs_back_off_and_terminalize_after_max_attempts() {
    let _guard = test_db_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let database_url = test_database_url();
    reset_database(&database_url);
    let mut conn =
        PgConnection::establish(&database_url).expect("test database should be reachable");

    let created_at = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    enqueue_publish_job(
        &mut conn,
        &NewPublishJob {
            nostr_event_id: "evt-backoff",
            nostr_pubkey: "npub1alice",
            event_created_at: created_at,
            event_payload: json!({"id": "evt-backoff"}),
            job_source: PublishJobSource::Live.as_str(),
            state: "pending",
        },
    )
    .expect("job should enqueue");

    let lease_expiry = Utc::now() + Duration::minutes(5);
    let claimed = claim_next_live_job(&mut conn, "worker-a", lease_expiry)
        .expect("claim should work")
        .expect("pending job should be claimable");
    assert_eq!(claimed.nostr_event_id, "evt-backoff");

    // A failed attempt keeps a backoff lease in the future so the claim
    // query skips the job instead of hot-looping on it.
    let before_failure = Utc::now();
    let failed = mark_publish_job_failed(&mut conn, "evt-backoff", "pds timeout")
        .expect("failed marker should work");
    assert_eq!(failed.state, "failed");
    let backoff_lease = failed
        .lease_expires_at
        .expect("failed job should keep a backoff lease");
    assert!(
        backoff_lease > before_failure,
        "backoff lease should be in the future"
    );
    assert!(
        backoff_lease <= before_failure + Duration::seconds(601),
        "backoff lease should be capped at 600 seconds"
    );

    // The backed-off job must not be claimable while the lease holds.
    let skipped =
        claim_next_live_job(&mut conn, "worker-a", lease_expiry).expect("claim should work");
    assert!(skipped.is_none(), "backed-off job should be skipped");

    // Once the backoff lease expires the job becomes claimable again.
    diesel::sql_query(
        "UPDATE publish_jobs
         SET lease_expires_at = NOW() - INTERVAL '1 second'
         WHERE nostr_event_id = 'evt-backoff'",
    )
    .execute(&mut conn)
    .expect("backoff lease should be forced expired");
    let reclaimed = claim_next_live_job(&mut conn, "worker-a", lease_expiry)
        .expect("claim should work")
        .expect("job with expired backoff should be reclaimable");
    assert_eq!(reclaimed.nostr_event_id, "evt-backoff");

    // Once the attempt cap is reached the job fails terminally and the
    // claim query never returns it again.
    diesel::sql_query("UPDATE publish_jobs SET attempt = 19 WHERE nostr_event_id = 'evt-backoff'")
        .execute(&mut conn)
        .expect("attempt count should be forced to the cap");
    let terminal = mark_publish_job_failed(&mut conn, "evt-backoff", "pds timeout")
        .expect("failed marker should work");
    assert_eq!(terminal.state, "failed");
    assert_eq!(terminal.attempt, 20);
    assert!(
        terminal.completed_at.is_some(),
        "terminally failed job should record completed_at"
    );
    assert!(terminal.lease_owner.is_none());
    assert!(terminal.lease_expires_at.is_none());
    let after_terminal =
        claim_next_live_job(&mut conn, "worker-a", lease_expiry).expect("claim should work");
    assert!(
        after_terminal.is_none(),
        "terminally failed job must never be reclaimed"
    );
}

#[test]
fn backfill_yields_the_daily_quota_to_live_posts() {
    // A catalog replay must not eat the daily upload allowance and starve the
    // user's fresh posts: on a quota rejection, backfill parks far longer than
    // live so live gets the next window.
    let _guard = test_db_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let database_url = test_database_url();
    reset_database(&database_url);
    let mut conn =
        PgConnection::establish(&database_url).expect("test database should be reachable");

    let created_at = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    enqueue_publish_job(
        &mut conn,
        &NewPublishJob {
            nostr_event_id: "evt-backfill-quota",
            nostr_pubkey: "npub1alice",
            event_created_at: created_at,
            event_payload: json!({"id": "evt-backfill-quota"}),
            job_source: PublishJobSource::Backfill.as_str(),
            state: "pending",
        },
    )
    .expect("job should enqueue");

    let quota_error =
        "uploadVideo failed (401): {\"jobStatus\":{\"error\":\"daily_vid_limit_exceeded\"}}";
    let before = Utc::now();
    let deferred = mark_publish_job_failed(&mut conn, "evt-backfill-quota", quota_error)
        .expect("quota failure should be recorded");

    let lease = deferred
        .lease_expires_at
        .expect("quota deferral must set a lease");
    assert!(
        lease > before + Duration::hours(6),
        "backfill should yield the quota for many hours, got {lease}"
    );
    assert!(deferred.completed_at.is_none(), "must stay reclaimable");
}
