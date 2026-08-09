use diesel::Connection;
use diesel::PgConnection;
use diesel::RunQueryDsl;
use divine_feedgen::{DbFeedStore, FeedStore};
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
        include_str!("../../../migrations/003_appview_read_model/down.sql"),
    );
    execute_batch(
        &mut conn,
        include_str!("../../../migrations/003_appview_read_model/up.sql"),
    );
}

fn seed_posts(database_url: &str) {
    let mut conn =
        PgConnection::establish(database_url).expect("test database should be reachable");
    diesel::sql_query(
        "INSERT INTO appview_posts (
            uri, did, rkey, created_at, text, search_text, deleted_at
         ) VALUES
            (
                'at://did:plc:feedgen/app.bsky.feed.post/old',
                'did:plc:feedgen',
                'old',
                '2026-08-09T10:00:00Z',
                'old post',
                'old post',
                NULL
            ),
            (
                'at://did:plc:feedgen/app.bsky.feed.post/newest',
                'did:plc:feedgen',
                'newest',
                '2026-08-09T12:00:00Z',
                'newest post',
                'newest post',
                NULL
            ),
            (
                'at://did:plc:feedgen/app.bsky.feed.post/deleted',
                'did:plc:feedgen',
                'deleted',
                '2026-08-09T13:00:00Z',
                'deleted post',
                'deleted post',
                '2026-08-09T13:30:00Z'
            )",
    )
    .execute(&mut conn)
    .expect("appview posts should insert");
}

fn keep_pool_alive_through_process_teardown(store: DbFeedStore) {
    // The coverage runner records the assertions but loses this test target's
    // data if libpq segfaults while the r2d2 pool drops during process teardown.
    std::mem::forget(store);
}

#[tokio::test]
#[serial]
async fn db_feed_store_lists_latest_posts_from_pool() {
    let database_url = test_database_url();
    reset_database(&database_url);
    seed_posts(&database_url);

    let store = DbFeedStore::connect(&database_url).expect("store should connect");

    let posts = store
        .latest_posts(1)
        .await
        .expect("latest posts should load");
    assert_eq!(
        posts,
        vec!["at://did:plc:feedgen/app.bsky.feed.post/newest"]
    );
    keep_pool_alive_through_process_teardown(store);
}

#[tokio::test]
#[serial]
async fn db_feed_store_lists_trending_posts_from_pool() {
    let database_url = test_database_url();
    reset_database(&database_url);
    seed_posts(&database_url);

    let store = DbFeedStore::connect(&database_url).expect("store should connect");

    let posts = store
        .trending_posts(10)
        .await
        .expect("trending posts should load");
    assert_eq!(
        posts,
        vec![
            "at://did:plc:feedgen/app.bsky.feed.post/newest",
            "at://did:plc:feedgen/app.bsky.feed.post/old",
        ]
    );
    keep_pool_alive_through_process_teardown(store);
}

#[tokio::test]
#[serial]
async fn db_feed_store_returns_empty_posts_from_pool() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let store = DbFeedStore::connect(&database_url).expect("store should connect");

    assert!(store
        .latest_posts(10)
        .await
        .expect("latest posts should load")
        .is_empty());
    assert!(store
        .trending_posts(10)
        .await
        .expect("trending posts should load")
        .is_empty());
    keep_pool_alive_through_process_teardown(store);
}
