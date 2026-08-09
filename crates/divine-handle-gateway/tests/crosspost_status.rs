use axum::body::to_bytes;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use diesel::Connection;
use diesel::PgConnection;
use diesel::RunQueryDsl;
use divine_handle_gateway::{app_with_config, AppConfig};
use serde_json::json;
use serde_json::Value;
use serial_test::serial;
use tower::util::ServiceExt;

const AUTH_HEADER: &str = "Bearer test-keycast-token";
const ALICE_PUBKEY: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const BOB_PUBKEY: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const ALICE_EVENT_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOB_EVENT_ID: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body to be readable");
    serde_json::from_slice(&bytes).expect("response to contain valid JSON")
}

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

fn build_app(database_url: String, name_server_url: String) -> axum::Router {
    let config = AppConfig {
        database_url,
        keycast_atproto_token: "test-keycast-token".to_string(),
        atproto_provisioning_url: format!("{name_server_url}/provision"),
        atproto_provisioning_token: None,
        atproto_keycast_sync_url: format!("{name_server_url}/api/internal/atproto/state"),
        atproto_name_server_sync_url: format!(
            "{name_server_url}/api/internal/username/set-atproto"
        ),
        atproto_name_server_sync_token: "test-sync-token".to_string(),
    };
    app_with_config(config).expect("test app should build")
}

fn insert_account(
    conn: &mut PgConnection,
    pubkey: &str,
    handle: &str,
    did: &str,
    provisioning_state: &str,
) {
    diesel::sql_query(
        "INSERT INTO account_links (
            nostr_pubkey, did, handle, crosspost_enabled, signing_key_id,
            plc_rotation_key_ref, provisioning_state
        ) VALUES ($1, $2, $3, TRUE, $4, $5, $6)",
    )
    .bind::<diesel::sql_types::Text, _>(pubkey)
    .bind::<diesel::sql_types::Text, _>(did)
    .bind::<diesel::sql_types::Text, _>(handle)
    .bind::<diesel::sql_types::Text, _>(format!("signing:{pubkey}"))
    .bind::<diesel::sql_types::Text, _>(format!("rotation:{pubkey}"))
    .bind::<diesel::sql_types::Text, _>(provisioning_state)
    .execute(conn)
    .unwrap();
}

fn insert_ready_account(conn: &mut PgConnection, pubkey: &str, handle: &str, did: &str) {
    insert_account(conn, pubkey, handle, did, "ready");
}

fn insert_publish_job(conn: &mut PgConnection, event_id: &str, pubkey: &str, state: &str) {
    diesel::sql_query(
        "INSERT INTO publish_jobs (
            nostr_event_id, nostr_pubkey, event_payload, job_source, state
        ) VALUES ($1, $2, '{}'::jsonb, 'live', $3)",
    )
    .bind::<diesel::sql_types::Text, _>(event_id)
    .bind::<diesel::sql_types::Text, _>(pubkey)
    .bind::<diesel::sql_types::Text, _>(state)
    .execute(conn)
    .unwrap();
}

fn insert_published_mapping(
    conn: &mut PgConnection,
    event_id: &str,
    did: &str,
    at_uri: &str,
    cid: &str,
) {
    diesel::sql_query(
        "INSERT INTO record_mappings (
            nostr_event_id, did, collection, rkey, at_uri, cid, status
        ) VALUES ($1, $2, 'app.bsky.feed.post', 'fullrkey', $3, $4, 'published')",
    )
    .bind::<diesel::sql_types::Text, _>(event_id)
    .bind::<diesel::sql_types::Text, _>(did)
    .bind::<diesel::sql_types::Text, _>(at_uri)
    .bind::<diesel::sql_types::Text, _>(cid)
    .execute(conn)
    .unwrap();
}

#[tokio::test]
#[serial]
async fn crosspost_status_requires_bearer_auth() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let name_server = mockito::Server::new_async().await;
    let app = build_app(database_url, name_server.url());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/account-links/{ALICE_PUBKEY}/crosspost-status"
                ))
                .header("content-type", "application/json")
                .body(Body::from(json!({"nostr_event_ids": []}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial]
async fn crosspost_status_rejects_over_cap_batches() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let name_server = mockito::Server::new_async().await;
    let app = build_app(database_url, name_server.url());
    let event_ids: Vec<String> = (0..101).map(|index| format!("{index:064x}")).collect();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/account-links/{ALICE_PUBKEY}/crosspost-status"
                ))
                .header("content-type", "application/json")
                .header("authorization", AUTH_HEADER)
                .body(Body::from(
                    json!({"nostr_event_ids": event_ids}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[serial]
async fn crosspost_status_unknown_account_reports_not_applicable() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let name_server = mockito::Server::new_async().await;
    let app = build_app(database_url, name_server.url());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/account-links/{ALICE_PUBKEY}/crosspost-status"
                ))
                .header("content-type", "application/json")
                .header("authorization", AUTH_HEADER)
                .body(Body::from(
                    json!({"nostr_event_ids": [ALICE_EVENT_ID]}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["account"]["provisioning_state"], "missing");
    assert_eq!(payload["videos"][0]["status"], "not_applicable");
}

#[tokio::test]
#[serial]
async fn crosspost_status_empty_batch_returns_empty_videos() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let name_server = mockito::Server::new_async().await;
    let app = build_app(database_url, name_server.url());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/account-links/{ALICE_PUBKEY}/crosspost-status"
                ))
                .header("content-type", "application/json")
                .header("authorization", AUTH_HEADER)
                .body(Body::from(json!({"nostr_event_ids": []}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["videos"], json!([]));
}

#[tokio::test]
#[serial]
async fn crosspost_status_non_ready_accounts_report_not_applicable() {
    let database_url = test_database_url();

    for (index, provisioning_state) in ["pending", "failed", "disabled"].into_iter().enumerate() {
        reset_database(&database_url);
        {
            let mut conn =
                PgConnection::establish(&database_url).expect("test database should be reachable");
            let pubkey = format!("{:064x}", index + 10);
            insert_account(
                &mut conn,
                &pubkey,
                &format!("state{index}.divine.video"),
                &format!("did:plc:state{index}"),
                provisioning_state,
            );
            insert_publish_job(&mut conn, ALICE_EVENT_ID, &pubkey, "pending");
        }

        let name_server = mockito::Server::new_async().await;
        let app = build_app(database_url.clone(), name_server.url());

        let pubkey = format!("{:064x}", index + 10);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/account-links/{pubkey}/crosspost-status"))
                    .header("content-type", "application/json")
                    .header("authorization", AUTH_HEADER)
                    .body(Body::from(
                        json!({"nostr_event_ids": [ALICE_EVENT_ID]}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let payload = response_json(response).await;
        assert_eq!(payload["account"]["provisioning_state"], provisioning_state);
        assert_eq!(payload["videos"][0]["status"], "not_applicable");
    }
}

#[tokio::test]
#[serial]
async fn crosspost_status_event_owned_by_another_pubkey_is_not_disclosed() {
    let database_url = test_database_url();
    reset_database(&database_url);

    {
        let mut conn =
            PgConnection::establish(&database_url).expect("test database should be reachable");
        insert_ready_account(
            &mut conn,
            ALICE_PUBKEY,
            "alice.divine.video",
            "did:plc:alice",
        );
        insert_ready_account(&mut conn, BOB_PUBKEY, "bob.divine.video", "did:plc:bob");
        insert_publish_job(&mut conn, BOB_EVENT_ID, BOB_PUBKEY, "published");
        insert_published_mapping(
            &mut conn,
            BOB_EVENT_ID,
            "did:plc:bob",
            "at://did:plc:bob/app.bsky.feed.post/bobfullrkey",
            "bafyreihiddenbobcid",
        );
    }

    let name_server = mockito::Server::new_async().await;
    let app = build_app(database_url, name_server.url());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/account-links/{ALICE_PUBKEY}/crosspost-status"
                ))
                .header("content-type", "application/json")
                .header("authorization", AUTH_HEADER)
                .body(Body::from(
                    json!({"nostr_event_ids": [BOB_EVENT_ID]}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["videos"][0]["status"], "not_applicable");
    assert!(payload["videos"][0]["at_uri"].is_null());
    assert!(payload["videos"][0]["cid"].is_null());
}

#[tokio::test]
#[serial]
async fn crosspost_status_published_row_returns_full_at_uri_and_cid() {
    let database_url = test_database_url();
    reset_database(&database_url);
    let at_uri = "at://did:plc:alice/app.bsky.feed.post/longstablefullrecordkey";
    let cid = "bafyreialicefullcidvaluewithouttruncation";

    {
        let mut conn =
            PgConnection::establish(&database_url).expect("test database should be reachable");
        insert_ready_account(
            &mut conn,
            ALICE_PUBKEY,
            "alice.divine.video",
            "did:plc:alice",
        );
        insert_publish_job(&mut conn, ALICE_EVENT_ID, ALICE_PUBKEY, "published");
        insert_published_mapping(&mut conn, ALICE_EVENT_ID, "did:plc:alice", at_uri, cid);
    }

    let name_server = mockito::Server::new_async().await;
    let app = build_app(database_url, name_server.url());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/account-links/{ALICE_PUBKEY}/crosspost-status"
                ))
                .header("content-type", "application/json")
                .header("authorization", AUTH_HEADER)
                .body(Body::from(
                    json!({"nostr_event_ids": [ALICE_EVENT_ID]}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["account"]["provisioning_state"], "ready");
    assert_eq!(payload["videos"][0]["status"], "published");
    assert_eq!(payload["videos"][0]["at_uri"], at_uri);
    assert_eq!(payload["videos"][0]["cid"], cid);
}

#[tokio::test]
#[serial]
async fn crosspost_status_returns_internal_error_on_account_store_failure() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let name_server = mockito::Server::new_async().await;
    let app = build_app(database_url.clone(), name_server.url());

    {
        let mut conn =
            PgConnection::establish(&database_url).expect("test database should be reachable");
        execute_batch(&mut conn, "DROP TABLE account_links CASCADE");
    }

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/account-links/{ALICE_PUBKEY}/crosspost-status"
                ))
                .header("content-type", "application/json")
                .header("authorization", AUTH_HEADER)
                .body(Body::from(
                    json!({"nostr_event_ids": [ALICE_EVENT_ID]}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
#[serial]
async fn crosspost_status_returns_internal_error_on_publish_status_query_failure() {
    let database_url = test_database_url();
    reset_database(&database_url);

    {
        let mut conn =
            PgConnection::establish(&database_url).expect("test database should be reachable");
        insert_ready_account(
            &mut conn,
            ALICE_PUBKEY,
            "alice.divine.video",
            "did:plc:alice",
        );
    }

    let name_server = mockito::Server::new_async().await;
    let app = build_app(database_url.clone(), name_server.url());

    {
        let mut conn =
            PgConnection::establish(&database_url).expect("test database should be reachable");
        execute_batch(&mut conn, "DROP TABLE publish_jobs");
    }

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/account-links/{ALICE_PUBKEY}/crosspost-status"
                ))
                .header("content-type", "application/json")
                .header("authorization", AUTH_HEADER)
                .body(Body::from(
                    json!({"nostr_event_ids": [ALICE_EVENT_ID]}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
#[serial]
async fn crosspost_status_mixed_batch_answers_in_request_order() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let queued_event_id = format!("{:064x}", 0x9ee1u128);
    let unknown_event_id = format!("{:064x}", 0x4e04u128);
    let at_uri = "at://did:plc:alice/app.bsky.feed.post/orderedrecordkey";
    let cid = "bafyreialiceorderedcidvalue";

    {
        let mut conn =
            PgConnection::establish(&database_url).expect("test database should be reachable");
        insert_ready_account(
            &mut conn,
            ALICE_PUBKEY,
            "alice.divine.video",
            "did:plc:alice",
        );
        insert_publish_job(&mut conn, ALICE_EVENT_ID, ALICE_PUBKEY, "published");
        insert_published_mapping(&mut conn, ALICE_EVENT_ID, "did:plc:alice", at_uri, cid);
        insert_publish_job(&mut conn, &queued_event_id, ALICE_PUBKEY, "pending");
    }

    let name_server = mockito::Server::new_async().await;
    let app = build_app(database_url, name_server.url());

    // Deliberately not sorted and not the row order the join returns: mobile
    // pairs each answer with the request slot it sent, so the response must
    // echo the requested sequence including IDs with no publish job at all.
    let requested = vec![
        queued_event_id.clone(),
        unknown_event_id.clone(),
        ALICE_EVENT_ID.to_string(),
    ];

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/account-links/{ALICE_PUBKEY}/crosspost-status"
                ))
                .header("content-type", "application/json")
                .header("authorization", AUTH_HEADER)
                .body(Body::from(
                    json!({"nostr_event_ids": requested}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    let videos = payload["videos"].as_array().expect("videos to be an array");

    assert_eq!(videos.len(), requested.len());
    for (index, expected_event_id) in requested.iter().enumerate() {
        assert_eq!(videos[index]["nostr_event_id"], *expected_event_id);
    }
    assert_eq!(videos[0]["status"], "queued");
    assert_eq!(videos[1]["status"], "not_applicable");
    assert_eq!(videos[2]["status"], "published");
    assert_eq!(videos[2]["at_uri"], at_uri);
    assert_eq!(videos[2]["cid"], cid);
}

#[tokio::test]
#[serial]
async fn crosspost_status_completed_job_without_mapping_reports_not_applicable() {
    let database_url = test_database_url();
    reset_database(&database_url);

    {
        let mut conn =
            PgConnection::establish(&database_url).expect("test database should be reachable");
        insert_ready_account(
            &mut conn,
            ALICE_PUBKEY,
            "alice.divine.video",
            "did:plc:alice",
        );
        // The pipeline reports a skip (unsupported kind, unverified signature, not
        // opted in) as successful completion, so the job lands in `published` with
        // no record mapping. Nothing reached Bluesky, so this must not read as a
        // post that was taken down.
        insert_publish_job(&mut conn, ALICE_EVENT_ID, ALICE_PUBKEY, "published");
    }

    let name_server = mockito::Server::new_async().await;
    let app = build_app(database_url, name_server.url());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/account-links/{ALICE_PUBKEY}/crosspost-status"
                ))
                .header("content-type", "application/json")
                .header("authorization", AUTH_HEADER)
                .body(Body::from(
                    json!({"nostr_event_ids": [ALICE_EVENT_ID]}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["videos"][0]["status"], "not_applicable");
    assert!(payload["videos"][0]["at_uri"].is_null());
}
