use std::collections::{HashMap, VecDeque};
use std::fs;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use divine_atbridge::nostr_consumer::{NostrConsumer, RelayConnection};
use divine_atbridge::pipeline::{
    AccountLink, AccountStore, AssetManifestRecord, BridgePipeline, HttpBlobFetcher, RecordMapping,
    RecordStore,
};
use divine_atbridge::publisher::PdsClient;
use divine_atbridge::run_bridge_session;
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

struct MockConnection {
    outgoing: Vec<String>,
    incoming: VecDeque<String>,
}

impl MockConnection {
    fn new(messages: Vec<String>) -> Self {
        Self {
            outgoing: Vec::new(),
            incoming: VecDeque::from(messages),
        }
    }
}

#[async_trait]
impl RelayConnection for MockConnection {
    async fn send(&mut self, msg: String) -> Result<()> {
        self.outgoing.push(msg);
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<String>> {
        Ok(self.incoming.pop_front())
    }

    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

struct StaticAccountStore {
    link: AccountLink,
}

#[async_trait]
impl AccountStore for StaticAccountStore {
    async fn get_account_link(&self, nostr_pubkey: &str) -> Result<Option<AccountLink>> {
        if nostr_pubkey == self.link.nostr_pubkey {
            Ok(Some(self.link.clone()))
        } else {
            Ok(None)
        }
    }
}

#[derive(Default)]
struct StatefulRecordStore {
    mappings: Mutex<HashMap<String, RecordMapping>>,
    manifests: Mutex<Vec<AssetManifestRecord>>,
    statuses: Mutex<Vec<(String, Option<String>, RecordStatus)>>,
    deleted: Mutex<Vec<String>>,
}

#[async_trait]
impl RecordStore for StatefulRecordStore {
    async fn is_event_processed(&self, event_id: &str) -> Result<bool> {
        Ok(self.mappings.lock().unwrap().contains_key(event_id))
    }

    async fn save_record_mapping(&self, mapping: RecordMapping) -> Result<()> {
        self.mappings
            .lock()
            .unwrap()
            .insert(mapping.nostr_event_id.clone(), mapping);
        Ok(())
    }

    async fn get_mapping_by_nostr_id(&self, event_id: &str) -> Result<Option<RecordMapping>> {
        Ok(self.mappings.lock().unwrap().get(event_id).cloned())
    }

    async fn mark_deleted(&self, event_id: &str) -> Result<()> {
        self.deleted.lock().unwrap().push(event_id.to_string());
        if let Some(mapping) = self.mappings.lock().unwrap().get_mut(event_id) {
            mapping.deleted = true;
        }
        Ok(())
    }

    async fn save_asset_manifest(&self, entry: AssetManifestRecord) -> Result<()> {
        self.manifests.lock().unwrap().push(entry);
        Ok(())
    }

    async fn update_record_mapping_status(
        &self,
        event_id: &str,
        cid: Option<&str>,
        status: RecordStatus,
    ) -> Result<()> {
        self.statuses
            .lock()
            .unwrap()
            .push((event_id.to_string(), cid.map(str::to_string), status));
        Ok(())
    }
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
        !compose.contains("tail -f /dev/null"),
        "bridge service should run the bridge process directly"
    );
}

#[tokio::test]
async fn e2e_local_stack_covers_publish_delete_replay_and_profile_sync() {
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
    let video_put = pds_server
        .mock("POST", "/xrpc/com.atproto.repo.putRecord")
        .match_body(mockito::Matcher::Regex(
            "app\\.bsky\\.feed\\.post".to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "uri": "at://did:plc:e2e/app.bsky.feed.post/e2e-video",
                "cid": "bafyrecorde2evideo"
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
            "Website: https://divine.video".to_string(),
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

    let frames = vec![
        serde_json::json!(["EVENT", "sub-1", publish_event.clone()]).to_string(),
        serde_json::json!(["EVENT", "sub-1", profile_event.clone()]).to_string(),
        serde_json::json!(["EVENT", "sub-1", delete_event.clone()]).to_string(),
    ];

    let pipeline = BridgePipeline::new(
        StaticAccountStore {
            link: AccountLink {
                nostr_pubkey: publish_event.pubkey.clone(),
                did: "did:plc:e2e".to_string(),
                opted_in: true,
            },
        },
        StatefulRecordStore::default(),
        HttpBlobFetcher::new(Duration::from_secs(5)).unwrap(),
        PdsClient::new(pds_server.url(), "e2e-token"),
        PdsClient::new(pds_server.url(), "e2e-token"),
    );

    let mut consumer = NostrConsumer::new("wss://relay.example".to_string());
    let mut connection = MockConnection::new(frames);
    run_bridge_session(&mut consumer, &mut connection, &pipeline)
        .await
        .unwrap();

    let published_statuses = pipeline.record_store.statuses.lock().unwrap().clone();
    assert!(published_statuses.iter().any(|(event_id, cid, status)| {
        event_id == &publish_event.id
            && cid.as_deref() == Some("bafyrecorde2evideo")
            && *status == RecordStatus::Published
    }));
    assert!(published_statuses.iter().any(|(event_id, cid, status)| {
        event_id == &profile_event.id
            && cid.as_deref() == Some("bafyrecordprofile")
            && *status == RecordStatus::Published
    }));

    assert_eq!(
        pipeline.record_store.deleted.lock().unwrap().as_slice(),
        std::slice::from_ref(&publish_event.id)
    );
    assert_eq!(consumer.last_seen_timestamp, Some(delete_event.created_at));

    let mut replay_connection = MockConnection::new(vec![]);
    run_bridge_session(&mut consumer, &mut replay_connection, &pipeline)
        .await
        .unwrap();
    let req: serde_json::Value = serde_json::from_str(&replay_connection.outgoing[0]).unwrap();
    assert_eq!(req[2]["since"], delete_event.created_at);

    video_put.assert_async().await;
    profile_put.assert_async().await;
    delete_mock.assert_async().await;
}
