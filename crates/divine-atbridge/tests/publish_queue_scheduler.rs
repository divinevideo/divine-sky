use chrono::{DateTime, Duration, Utc};
use diesel::Connection;
use diesel::PgConnection;
use diesel::RunQueryDsl;
use divine_bridge_db::models::NewPublishJob;
use divine_bridge_db::{
    cancel_publish_job, claim_next_backfill_job, claim_next_live_job, enqueue_publish_job,
    get_publish_job, list_accounts_requiring_backfill, mark_account_backfill_completed,
    mark_account_backfill_failed, mark_account_backfill_started, mark_publish_job_completed,
    mark_publish_job_failed,
};
use divine_bridge_types::PublishJobSource;
use serde_json::json;

fn test_database_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://divine:divine_dev@[::1]:5432/divine_bridge".to_string())
}

fn execute_batch(conn: &mut PgConnection, sql: &str) {
    for statement in sql
        .split(';')
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        diesel::sql_query(statement).execute(conn).unwrap();
    }
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
}

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
fn queue_and_backfill_queries_follow_scheduler_contract() {
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
