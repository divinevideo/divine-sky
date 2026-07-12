use std::process::Command;
use std::sync::{Mutex, OnceLock};

use chrono::{Duration, Utc};
use diesel::sql_types::{Bool, Jsonb, Nullable, Text, Timestamptz};
use diesel::{Connection, PgConnection, RunQueryDsl};
use divine_atbridge::legacy_repair::LegacyRepairService;
use divine_bridge_db::{
    claim_next_live_job, preview_legacy_badjwt_repair, revive_legacy_badjwt_jobs,
    run_pending_migrations_on, LegacyBadJwtRepairFilter,
};
use serde_json::json;

const ACCOUNT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MATCHING_EVENT: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const NEAR_MISS_EVENT: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const BADJWT: &str = "BadJwt: Signature tag didn't verify";
const STORED_BADJWT: &str = "failed to upload blob to PDS: failed to get service auth for video \
upload: getServiceAuth failed (400) for did:plc:repair-test: BadJwt: Signature tag didn't verify";

fn test_database_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://divine:divine_dev@[::1]:5432/divine_bridge".to_string())
}

fn test_db_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn setup() -> PgConnection {
    let mut conn =
        PgConnection::establish(&test_database_url()).expect("test database should be reachable");
    run_pending_migrations_on(&mut conn).expect("migrations should run");
    diesel::sql_query("TRUNCATE operator_actions, publish_jobs, account_links CASCADE")
        .execute(&mut conn)
        .expect("tables should truncate");

    diesel::sql_query(
        "INSERT INTO account_links (
            nostr_pubkey, did, handle, crosspost_enabled, signing_key_id,
            plc_rotation_key_ref, provisioning_state
         ) VALUES ($1, $2, $3, $4, 'key', 'rotation', 'ready')",
    )
    .bind::<Text, _>(ACCOUNT)
    .bind::<Nullable<Text>, _>(Some("did:plc:repair-test"))
    .bind::<Text, _>("repair-test.divine.video")
    .bind::<Bool, _>(true)
    .execute(&mut conn)
    .expect("account should insert");

    seed_terminal_job(&mut conn, MATCHING_EVENT, STORED_BADJWT);
    seed_terminal_job(
        &mut conn,
        NEAR_MISS_EVENT,
        "wrapper: BadJwt: Signature tag didn't verify",
    );
    conn
}

fn seed_terminal_job(conn: &mut PgConnection, event_id: &str, error: &str) {
    diesel::sql_query(
        "INSERT INTO publish_jobs (
            nostr_event_id, nostr_pubkey, event_created_at, event_payload,
            job_source, attempt, state, error, completed_at
         ) VALUES ($1, $2, $3, $4, 'live', 20, 'failed', $5, NOW())",
    )
    .bind::<Text, _>(event_id)
    .bind::<Text, _>(ACCOUNT)
    .bind::<Timestamptz, _>(Utc::now())
    .bind::<Jsonb, _>(json!({"id": event_id, "kind": 34236}))
    .bind::<Text, _>(error)
    .execute(conn)
    .expect("job should insert");
}

#[test]
fn preview_and_revival_select_only_exact_terminal_badjwt_jobs() {
    let _guard = test_db_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut conn = setup();
    let filter = LegacyBadJwtRepairFilter {
        nostr_pubkey: ACCOUNT.to_string(),
        event_ids: Vec::new(),
        exact_error: Some(BADJWT.to_string()),
        after_event_id: None,
        limit: 100,
    };

    let preview = preview_legacy_badjwt_repair(&mut conn, &filter).expect("preview should work");
    assert_eq!(preview.total_matching, 1);
    assert!(!preview.has_more);
    assert_eq!(preview.jobs.len(), 1);
    assert_eq!(preview.jobs[0].nostr_event_id, MATCHING_EVENT);

    let changed = revive_legacy_badjwt_jobs(&mut conn, &filter, &[MATCHING_EVENT.to_string()])
        .expect("revival should work");
    assert_eq!(changed, 1);

    let claimed = claim_next_live_job(
        &mut conn,
        "repair-test-worker",
        Utc::now() + Duration::minutes(5),
    )
    .expect("claim should work")
    .expect("repaired job should be claimable");
    assert_eq!(claimed.nostr_event_id, MATCHING_EVENT);
}

#[test]
fn confirmed_repair_is_bound_to_the_persisted_preview() {
    let _guard = test_db_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    drop(setup());
    let service = LegacyRepairService::new(test_database_url());
    let filter = LegacyBadJwtRepairFilter {
        nostr_pubkey: ACCOUNT.to_string(),
        event_ids: Vec::new(),
        exact_error: Some(BADJWT.to_string()),
        after_event_id: None,
        limit: 100,
    };

    let preview = service
        .preview("operator@example.com", filter)
        .expect("preview should persist");
    assert_eq!(preview.matched_event_ids, vec![MATCHING_EVENT]);

    let wrong_digest = service.confirm(&preview.operation_id, "wrong");
    assert!(wrong_digest.is_err());

    let applied = service
        .confirm(&preview.operation_id, &preview.confirmation_digest)
        .expect("matching confirmation should apply");
    assert_eq!(applied.changed_count, 1);

    let repeated = service
        .confirm(&preview.operation_id, &preview.confirmation_digest)
        .expect("confirmation should be idempotent");
    assert_eq!(repeated.changed_count, 1);
}

#[test]
fn rollback_restores_an_unclaimed_repair() {
    let _guard = test_db_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    drop(setup());
    let service = LegacyRepairService::new(test_database_url());
    let preview = service
        .preview(
            "operator@example.com",
            LegacyBadJwtRepairFilter {
                nostr_pubkey: ACCOUNT.to_string(),
                event_ids: vec![MATCHING_EVENT.to_string()],
                exact_error: None,
                after_event_id: None,
                limit: 1,
            },
        )
        .expect("preview should persist");
    service
        .confirm(&preview.operation_id, &preview.confirmation_digest)
        .expect("confirmation should apply");

    let rollback = service
        .rollback(&preview.operation_id)
        .expect("unclaimed repair should roll back");
    assert_eq!(rollback.changed_count, 1);
    assert_eq!(rollback.status, "rolled_back");

    let mut conn = PgConnection::establish(&test_database_url()).expect("database should connect");
    let claimed = claim_next_live_job(
        &mut conn,
        "repair-test-worker",
        Utc::now() + Duration::minutes(5),
    )
    .expect("claim should work");
    assert!(claimed.is_none());
}

#[test]
fn preview_rejects_combining_explicit_ids_with_badjwt_class() {
    let _guard = test_db_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    drop(setup());
    let service = LegacyRepairService::new(test_database_url());
    let result = service.preview(
        "operator@example.com",
        LegacyBadJwtRepairFilter {
            nostr_pubkey: ACCOUNT.to_string(),
            event_ids: vec![MATCHING_EVENT.to_string()],
            exact_error: Some(BADJWT.to_string()),
            after_event_id: None,
            limit: 100,
        },
    );
    assert!(result.is_err());
}

#[test]
fn repair_cli_help_does_not_require_runtime_secrets() {
    let output = Command::new(repair_binary())
        .arg("--help")
        .output()
        .expect("repair CLI should start");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("help should be UTF-8");
    assert!(stdout.contains("--actor"));
    assert!(stdout.contains("--confirm-digest"));
    assert!(stdout.contains("--rollback-operation-id"));
    assert!(!stdout.contains("DATABASE_URL="));
}

fn repair_binary() -> String {
    std::env::var("CARGO_BIN_EXE_repair-legacy-badjwt")
        .expect("Cargo should expose the repair binary")
}

fn run_repair_cli(args: &[&str]) -> std::process::Output {
    Command::new(repair_binary())
        .args(args)
        .env("DATABASE_URL", test_database_url())
        .output()
        .expect("repair CLI should run")
}

#[test]
fn repair_cli_previews_confirms_and_rolls_back() {
    let _guard = test_db_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    drop(setup());

    let preview_output = run_repair_cli(&[
        "--actor",
        "operator@example.com",
        "--nostr-pubkey",
        ACCOUNT,
        "--exact-badjwt",
        "--max-rows",
        "1",
    ]);
    assert!(preview_output.status.success());
    let preview: serde_json::Value =
        serde_json::from_slice(&preview_output.stdout).expect("preview should be JSON");
    assert_eq!(preview["matched_event_ids"], json!([MATCHING_EVENT]));

    let operation_id = preview["operation_id"].as_str().expect("operation ID");
    let digest = preview["confirmation_digest"].as_str().expect("digest");
    let confirm_output =
        run_repair_cli(&["--operation-id", operation_id, "--confirm-digest", digest]);
    assert!(confirm_output.status.success());

    let rollback_output = run_repair_cli(&["--rollback-operation-id", operation_id]);
    assert!(rollback_output.status.success());
    let rollback: serde_json::Value =
        serde_json::from_slice(&rollback_output.stdout).expect("rollback should be JSON");
    assert_eq!(rollback["status"], "rolled_back");
}

#[test]
fn atbridge_image_contains_the_repair_binary() {
    let dockerfile = include_str!("../../../Dockerfile.atbridge");
    assert!(dockerfile.contains("--bin repair-legacy-badjwt"));
    assert!(dockerfile.contains(
        "COPY --from=builder /app/target/release/repair-legacy-badjwt /usr/local/bin/repair-legacy-badjwt"
    ));
}
