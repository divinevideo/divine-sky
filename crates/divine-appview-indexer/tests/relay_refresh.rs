use std::collections::{HashMap, VecDeque};

use anyhow::Result;
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use divine_appview_indexer::pds_client::{PdsSource, RepoSnapshot};
use divine_appview_indexer::relay::RelayStream;
use divine_appview_indexer::store::{IndexedPost, IndexedRepo, MemoryStore};
use divine_appview_indexer::sync::run_single_event_loop;

struct FakeRelayStream {
    changed_dids: VecDeque<String>,
}

#[async_trait]
impl RelayStream for FakeRelayStream {
    async fn next_changed_repo(&mut self) -> Result<Option<String>> {
        Ok(self.changed_dids.pop_front())
    }
}

struct FakePdsClient {
    snapshots: HashMap<String, RepoSnapshot>,
}

#[async_trait]
impl PdsSource for FakePdsClient {
    async fn list_repos(&self) -> Result<Vec<IndexedRepo>> {
        Ok(self
            .snapshots
            .values()
            .map(|snapshot| snapshot.repo.clone())
            .collect())
    }

    async fn sync_repo(&self, did: &str) -> Result<RepoSnapshot> {
        Ok(self.snapshots.get(did).cloned().unwrap())
    }
}

#[tokio::test]
async fn relay_event_triggers_repo_resync_and_media_queueing() {
    let did = "did:plc:ebt5msdpfavoklkap6gl54bm";
    let indexed_at = Utc.with_ymd_and_hms(2026, 3, 21, 12, 0, 0).unwrap();
    let snapshot = RepoSnapshot {
        repo: IndexedRepo {
            did: did.to_string(),
            handle: Some("divine.test".to_string()),
            head: None,
            rev: None,
            active: true,
            last_backfilled_at: None,
            last_seen_seq: None,
        },
        profile: None,
        posts: vec![IndexedPost {
            uri: format!("at://{did}/app.bsky.feed.post/MA6mjTWZKEB"),
            did: did.to_string(),
            rkey: "MA6mjTWZKEB".to_string(),
            record_cid: Some("cid-demo".to_string()),
            created_at: indexed_at,
            text: "relay".to_string(),
            langs_json: None,
            embed_blob_cid: Some("bafkrei-relay".to_string()),
            embed_alt: None,
            aspect_ratio_width: Some(9),
            aspect_ratio_height: Some(16),
            raw_json: None,
            search_text: "relay".to_string(),
            indexed_at,
            deleted_at: None,
        }],
    };
    let pds = FakePdsClient {
        snapshots: HashMap::from([(did.to_string(), snapshot)]),
    };
    let mut relay = FakeRelayStream {
        changed_dids: VecDeque::from(vec![did.to_string()]),
    };
    let store = MemoryStore::default();

    run_single_event_loop(&mut relay, &pds, &store)
        .await
        .unwrap();

    assert_eq!(store.media_jobs().len(), 1);
}
