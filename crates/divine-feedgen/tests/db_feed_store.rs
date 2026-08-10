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

// r2d2 replenishes toward `min_idle` in the background, so the last checkout can
// leave a `PQconnectdb` running on the pool's thread pool. That handshake is
// inside OpenSSL, and libc runs `OPENSSL_cleanup` as soon as the test binary
// reaches `exit()`; the two race and segfault the process after the assertions
// have already passed.
//
// Call this once nothing in the test still holds a pool. Dropping first is what
// stops r2d2 scheduling more connects, because a queued `add_connection` job
// only holds a `Weak` to the pool; this wait is for the one that already
// started.
async fn settle_pool_teardown() {
    // An in-flight connect was measured to finish well inside 25ms even with
    // every core saturated. Keep an order of magnitude of headroom for CI.
    const SETTLE: std::time::Duration = std::time::Duration::from_millis(250);
    tokio::time::sleep(SETTLE).await;
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
    drop(store);
    settle_pool_teardown().await;
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
    drop(store);
    settle_pool_teardown().await;
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
    drop(store);
    settle_pool_teardown().await;
}
