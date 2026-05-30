use std::fs;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use diesel::Connection;
use diesel::PgConnection;
use diesel::RunQueryDsl;
use divine_atbridge::pipeline::{BridgePipeline, HttpBlobFetcher};
use divine_atbridge::publisher::PdsClient;
use divine_atbridge::runtime::{
    enqueue_live_event, run_publish_worker_once, DbAccountStore, DbRecordStore, SharedConnection,
    WorkerRunResult,
};
use divine_bridge_db::{get_ingest_offset, get_publish_job, get_record_mapping};
use divine_bridge_types::{NostrEvent, RecordStatus};
use secp256k1::rand::rngs::OsRng;
use secp256k1::{Keypair, Secp256k1};
use sha2::{Digest, Sha256};

fn make_signed_event_with_keypair(
    keypair: &Keypair,
    kind: u64,
    created_at: i64,
    content: &str,
    tags: Vec<Vec<String>>,
) -> NostrEvent {
    let secp = Secp256k1::new();
    let (xonly, _) = keypair.x_only_public_key();
    let pubkey_hex = hex::encode(xonly.serialize());

    let canonical = serde_json::json!([0, pubkey_hex, created_at, kind, tags, content]);
    let canonical_bytes = serde_json::to_string(&canonical).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(canonical_bytes.as_bytes());
    let id_bytes: [u8; 32] = hasher.finalize().into();
    let id_hex = hex::encode(id_bytes);

    let msg = secp256k1::Message::from_digest(id_bytes);
    let sig = secp.sign_schnorr(&msg, keypair);
    let sig_hex = hex::encode(sig.serialize());

    NostrEvent {
        id: id_hex,
        pubkey: pubkey_hex,
        created_at,
        kind,
        tags,
        content: content.to_string(),
        sig: sig_hex,
    }
}

fn make_video_event(keypair: &Keypair, created_at: i64, url: &str, sha256: &str) -> NostrEvent {
    make_signed_event_with_keypair(
        keypair,
        34235,
        created_at,
        "e2e publish",
        vec![
            vec!["title".into(), "E2E Publish".into()],
            vec!["url".into(), url.into()],
            vec!["x".into(), sha256.into()],
            vec!["d".into(), "e2e-video".into()],
        ],
    )
}

fn make_delete_event(keypair: &Keypair, created_at: i64, target_id: &str) -> NostrEvent {
    make_signed_event_with_keypair(
        keypair,
        5,
        created_at,
        "",
        vec![vec!["e".into(), target_id.into()]],
    )
}

fn make_profile_event(
    keypair: &Keypair,
    created_at: i64,
    avatar_url: &str,
    banner_url: &str,
) -> NostrEvent {
    make_signed_event_with_keypair(
        keypair,
        0,
        created_at,
        &serde_json::json!({
            "display_name": "DiVine Creator",
            "about": "Cross-posted bio",
            "picture": avatar_url,
            "banner": banner_url,
            "website": "https://divine.video"
        })
        .to_string(),
        vec![],
    )
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

fn test_db_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn shared_connection(database_url: &str) -> SharedConnection {
    Arc::new(Mutex::new(
        PgConnection::establish(database_url).expect("test database should be reachable"),
    ))
}

fn insert_ready_account(conn: &mut PgConnection, nostr_pubkey: &str, did: &str, handle: &str) {
    diesel::sql_query(format!(
        "INSERT INTO account_links (
            nostr_pubkey, did, handle, crosspost_enabled, signing_key_id,
            plc_rotation_key_ref, provisioning_state, provisioning_error,
            publish_backfill_state, publish_backfill_started_at,
            publish_backfill_completed_at, publish_backfill_error,
            disabled_at
        ) VALUES (
            '{nostr_pubkey}', '{did}', '{handle}', TRUE, 'signing-{nostr_pubkey}',
            'rotation-{nostr_pubkey}', 'ready', NULL, 'not_started', NULL, NULL, NULL, NULL
        )"
    ))
    .execute(conn)
    .expect("account should insert");
}

#[test]
fn e2e_local_stack_defines_required_services_and_healthchecks() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crate should live under repo root");
    let compose = fs::read_to_string(repo_root.join("config/docker-compose.yml"))
        .expect("config/docker-compose.yml should exist");
    let pds_compose = fs::read_to_string(repo_root.join("deploy/pds/docker-compose.yml"))
        .expect("deploy/pds/docker-compose.yml should exist");
    let pds_env = fs::read_to_string(repo_root.join("deploy/pds/env.example"))
        .expect("deploy/pds/env.example should exist");
    let minio_init = fs::read_to_string(repo_root.join("config/minio-init.sh"))
        .expect("config/minio-init.sh should exist");
    let mock_blossom = fs::read_to_string(repo_root.join("config/mock-blossom/server.py"))
        .expect("config/mock-blossom/server.py should exist");

    for service in [
        "postgres:",
        "minio:",
        "minio-init:",
        "mock-blossom:",
        "mock-relay:",
        "pds:",
        "bridge:",
    ] {
        assert!(
            compose.contains(service),
            "missing {service} in local compose"
        );
    }
    assert!(
        compose.contains("healthcheck:"),
        "local compose should define healthchecks"
    );
    assert!(
        pds_compose.contains("healthcheck:"),
        "pds compose should keep healthchecks"
    );
    assert!(
        minio_init.contains("mc mb --ignore-existing"),
        "bucket bootstrap should create buckets"
    );
    assert!(
        mock_blossom.contains("BaseHTTPRequestHandler"),
        "mock blossom server should be executable"
    );
    assert!(
        compose.contains("RELAY_URL: ws://mock-relay:8765"),
        "bridge service should point at the local mock relay"
    );
    assert!(
        compose.contains("PDS_AUTH_TOKEN: local-dev-token"),
        "bridge service should provide an explicit PDS auth token"
    );
    assert!(
        compose.contains("PDS_BLOBSTORE_S3_BUCKET: pds-blobs"),
        "local compose should pin the PDS blob bucket name"
    );
    assert!(
        compose.contains("AWS_ENDPOINT_BUCKET: pds-blobs"),
        "local compose should expose the endpoint bucket name for blob copy operations"
    );
    assert!(
        compose.contains("build:"),
        "local compose should build the patched rsky-pds image"
    );
    assert!(
        compose.contains("context: ../../rsky"),
        "local compose should build rsky-pds from the sibling fork checkout"
    );
    assert!(
        pds_compose.contains("PDS_BLOBSTORE_S3_BUCKET=${PDS_BLOBSTORE_S3_BUCKET:-pds-blobs}"),
        "standalone pds compose should pin the PDS blob bucket name"
    );
    assert!(
        pds_compose.contains("AWS_ENDPOINT_BUCKET=${AWS_ENDPOINT_BUCKET:-pds-blobs}"),
        "standalone pds compose should expose the endpoint bucket name"
    );
    assert!(
        pds_compose.contains("build:"),
        "standalone pds compose should build the patched rsky-pds image"
    );
    assert!(
        pds_compose.contains("context: ../../../rsky"),
        "standalone pds compose should build rsky-pds from the sibling fork checkout"
    );
    assert!(
        pds_env.contains("PDS_BLOBSTORE_S3_BUCKET=pds-blobs"),
        "pds env example should document the required blob bucket setting"
    );
    assert!(
        pds_env.contains("AWS_ENDPOINT_BUCKET=pds-blobs"),
        "pds env example should document the endpoint bucket setting"
    );
    assert!(
        !compose.contains("tail -f /dev/null"),
        "bridge service should run the bridge process directly"
    );
}

#[tokio::test]
async fn e2e_local_stack_scheduler_publishes_profiles_posts_and_deletes() {
    let _guard = test_db_lock().lock().unwrap();
    let database_url = test_database_url();
    reset_database(&database_url);
    let connection = shared_connection(&database_url);

    let secp = Secp256k1::new();
    let keypair = Keypair::new(&secp, &mut OsRng);
    let mut blossom_server = mockito::Server::new_async().await;
    let video_bytes = b"e2e-video".to_vec();
    let video_sha256 = hex::encode(Sha256::digest(&video_bytes));

    blossom_server
        .mock("GET", "/video/e2e.mp4")
        .with_status(200)
        .with_header("content-type", "video/mp4")
        .with_body(video_bytes.clone())
        .create_async()
        .await;
    blossom_server
        .mock("GET", "/profile/avatar.png")
        .with_status(200)
        .with_header("content-type", "image/png")
        .with_body(b"avatar-bytes".as_slice())
        .create_async()
        .await;
    blossom_server
        .mock("GET", "/profile/banner.png")
        .with_status(200)
        .with_header("content-type", "image/png")
        .with_body(b"banner-bytes".as_slice())
        .create_async()
        .await;

    let mut pds_server = mockito::Server::new_async().await;
    pds_server
        .mock("POST", "/xrpc/com.atproto.repo.uploadBlob")
        .expect(3)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "blob": {
                    "$type": "blob",
                    "ref": {"$link": "bafkreie2eblob"},
                    "mimeType": "application/octet-stream",
                    "size": 10
                }
            })
            .to_string(),
        )
        .create_async()
        .await;
    let video_create = pds_server
        .mock("POST", "/xrpc/com.atproto.repo.createRecord")
        .match_request(|request| {
            let body: serde_json::Value =
                serde_json::from_str(&request.utf8_lossy_body().unwrap()).unwrap();
            body["collection"] == "app.bsky.feed.post"
                && body["validate"] == true
                && body.get("rkey").is_none()
        })
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "uri": "at://did:plc:e2e/app.bsky.feed.post/e2e-video",
                "cid": "bafyrecorde2evideo",
                "validationStatus": "valid"
            })
            .to_string(),
        )
        .create_async()
        .await;
    let profile_put = pds_server
        .mock("POST", "/xrpc/com.atproto.repo.putRecord")
        .match_body(mockito::Matcher::Regex(
            "app\\.bsky\\.actor\\.profile".to_string(),
        ))
        .match_body(mockito::Matcher::Regex(
            "\"website\":\"https://divine.video\"".to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "uri": "at://did:plc:e2e/app.bsky.actor.profile/self",
                "cid": "bafyrecordprofile"
            })
            .to_string(),
        )
        .create_async()
        .await;
    let delete_mock = pds_server
        .mock("POST", "/xrpc/com.atproto.repo.deleteRecord")
        .match_body(mockito::Matcher::Regex("e2e-video".to_string()))
        .with_status(200)
        .with_body("{}")
        .create_async()
        .await;

    let publish_event = make_video_event(
        &keypair,
        1_700_000_100,
        &format!("{}/video/e2e.mp4", blossom_server.url()),
        &video_sha256,
    );
    let profile_event = make_profile_event(
        &keypair,
        1_700_000_101,
        &format!("{}/profile/avatar.png", blossom_server.url()),
        &format!("{}/profile/banner.png", blossom_server.url()),
    );
    let delete_event = make_delete_event(&keypair, 1_700_000_102, &publish_event.id);
    {
        let mut conn = connection.lock().unwrap();
        insert_ready_account(
            &mut conn,
            &publish_event.pubkey,
            "did:plc:e2e",
            "e2e.divine.video",
        );
    }

    let pipeline = BridgePipeline::new(
        DbAccountStore::new(connection.clone()),
        DbRecordStore::new(connection.clone()),
        HttpBlobFetcher::new(Duration::from_secs(5)).unwrap(),
        PdsClient::new(pds_server.url(), "e2e-token"),
        PdsClient::new(pds_server.url(), "e2e-token"),
    );

    enqueue_live_event(&connection, "runtime-e2e", &pipeline, &publish_event)
        .await
        .expect("publish event should enqueue");
    let publish_result = run_publish_worker_once(
        &connection,
        &pipeline,
        divine_bridge_types::PublishJobSource::Live,
        "runtime-live-worker",
    )
    .await
    .expect("publish worker should complete");
    assert!(matches!(
        publish_result,
        WorkerRunResult::Completed { ref nostr_event_id } if nostr_event_id == &publish_event.id
    ));

    enqueue_live_event(&connection, "runtime-e2e", &pipeline, &profile_event)
        .await
        .expect("profile event should enqueue");
    let profile_result = run_publish_worker_once(
        &connection,
        &pipeline,
        divine_bridge_types::PublishJobSource::Live,
        "runtime-live-worker",
    )
    .await
    .expect("profile worker should complete");
    assert!(matches!(
        profile_result,
        WorkerRunResult::Completed { ref nostr_event_id } if nostr_event_id == &profile_event.id
    ));

    enqueue_live_event(&connection, "runtime-e2e", &pipeline, &delete_event)
        .await
        .expect("delete event should enqueue/cancel");
    let delete_job = {
        let mut conn = connection.lock().unwrap();
        get_publish_job(&mut conn, &delete_event.id)
            .expect("delete job lookup should succeed")
            .expect("delete execution job should be queued")
    };
    assert_eq!(delete_job.state, "pending");

    let delete_result = run_publish_worker_once(
        &connection,
        &pipeline,
        divine_bridge_types::PublishJobSource::Live,
        "runtime-live-worker",
    )
    .await
    .expect("delete worker should complete");
    assert!(matches!(
        delete_result,
        WorkerRunResult::Completed { ref nostr_event_id } if nostr_event_id == &delete_event.id
    ));

    let mut conn = connection.lock().unwrap();
    let published_mapping = get_record_mapping(&mut conn, &publish_event.id)
        .expect("publish mapping lookup should succeed")
        .expect("publish mapping should exist");
    assert_eq!(published_mapping.status, RecordStatus::Deleted.as_str());

    let publish_job = get_publish_job(&mut conn, &publish_event.id)
        .expect("publish job lookup should succeed")
        .expect("publish job should exist");
    let profile_job = get_publish_job(&mut conn, &profile_event.id)
        .expect("profile job lookup should succeed")
        .expect("profile job should exist");
    let delete_job = get_publish_job(&mut conn, &delete_event.id)
        .expect("delete job lookup should succeed")
        .expect("delete job should exist");
    assert_eq!(publish_job.state, "published");
    assert_eq!(profile_job.state, "published");
    assert_eq!(delete_job.state, "published");

    let cursor = get_ingest_offset(&mut conn, "runtime-e2e")
        .expect("cursor lookup should succeed")
        .expect("cursor should exist");
    assert_eq!(cursor.last_event_id, delete_event.id);
    assert_eq!(cursor.last_created_at.timestamp(), delete_event.created_at);

    video_create.assert_async().await;
    profile_put.assert_async().await;
    delete_mock.assert_async().await;
}
