use diesel::Connection;
use diesel::PgConnection;
use diesel::RunQueryDsl;
use divine_bridge_db::models::NewLabelerEvent;
use divine_labeler::store::DbStore;
use serial_test::serial;

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
        include_str!("../../../migrations/002_label_tracking/down.sql"),
    );
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
        include_str!("../../../migrations/002_label_tracking/up.sql"),
    );
}

fn seed_record_mapping(database_url: &str) {
    let mut conn =
        PgConnection::establish(database_url).expect("test database should be reachable");
    execute_batch(
        &mut conn,
        "INSERT INTO account_links (
            nostr_pubkey, did, handle, crosspost_enabled, signing_key_id,
            plc_rotation_key_ref, provisioning_state, provisioning_error, disabled_at
         ) VALUES (
            'npub1labelerstore',
            'did:plc:labelerstore',
            'labelerstore.test',
            true,
            'signing-key',
            'rotation-key',
            'ready',
            NULL,
            NULL
         );
         INSERT INTO record_mappings (
            nostr_event_id, did, collection, rkey, at_uri, cid, status
         ) VALUES (
            'nostr-event-id',
            'did:plc:labelerstore',
            'app.bsky.feed.post',
            'post-rkey',
            'at://did:plc:labelerstore/app.bsky.feed.post/post-rkey',
            'bafy-test-cid',
            'published'
         )",
    );
}

fn keep_pool_alive_through_process_teardown(store: DbStore) {
    // The coverage runner records the assertions but loses this test target's
    // data if libpq segfaults while the r2d2 pool drops during process teardown.
    std::mem::forget(store);
}

#[tokio::test]
#[serial]
async fn db_store_exercises_labeler_event_queries_through_pool() {
    let database_url = test_database_url();
    reset_database(&database_url);
    seed_record_mapping(&database_url);

    let store = DbStore::connect(&database_url).expect("store should connect");
    assert_eq!(store.get_latest_seq().await.unwrap(), None);

    let first = store
        .insert_labeler_event(&NewLabelerEvent {
            src_did: "did:plc:test-labeler",
            subject_uri: "at://sha256:first",
            subject_cid: Some("bafy-first"),
            val: "porn",
            neg: false,
            nostr_event_id: Some("nostr-first"),
            sha256: Some("sha256-first"),
            origin: "divine",
        })
        .await
        .expect("first label should insert");
    let second = store
        .insert_labeler_event(&NewLabelerEvent {
            src_did: "did:plc:test-labeler",
            subject_uri: "at://sha256:second",
            subject_cid: None,
            val: "!takedown",
            neg: false,
            nostr_event_id: Some("nostr-second"),
            sha256: Some("sha256-second"),
            origin: "human",
        })
        .await
        .expect("second label should insert");

    assert_eq!(
        store
            .get_at_uri_by_event_id("nostr-event-id")
            .await
            .expect("record mapping lookup should load"),
        Some((
            "at://did:plc:labelerstore/app.bsky.feed.post/post-rkey".to_string(),
            "did:plc:labelerstore".to_string()
        ))
    );
    assert_eq!(
        store
            .get_at_uri_by_event_id("missing-event-id")
            .await
            .expect("missing record mapping lookup should load"),
        None
    );
    assert_eq!(
        store
            .get_latest_seq()
            .await
            .expect("latest seq should load"),
        Some(second.seq)
    );

    let events = store
        .get_events_after(first.seq, 10)
        .await
        .expect("events after first seq should load");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].seq, second.seq);
    assert_eq!(events[0].val, "!takedown");
    keep_pool_alive_through_process_teardown(store);
}
