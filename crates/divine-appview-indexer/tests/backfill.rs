use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use divine_appview_indexer::pds_client::{PdsSource, RepoSnapshot};
use divine_appview_indexer::store::{IndexedPost, IndexedProfile, IndexedRepo, MemoryStore};
use divine_appview_indexer::sync::sync_repo_from_pds;

struct FakePdsClient {
    repos: Vec<IndexedRepo>,
    snapshots: HashMap<String, RepoSnapshot>,
}

#[async_trait]
impl PdsSource for FakePdsClient {
    async fn list_repos(&self) -> Result<Vec<IndexedRepo>> {
        Ok(self.repos.clone())
    }

    async fn sync_repo(&self, did: &str) -> Result<RepoSnapshot> {
        Ok(self.snapshots.get(did).cloned().unwrap())
    }
}

#[tokio::test]
async fn backfill_syncs_profiles_posts_and_blob_metadata_from_pds() {
    let did = "did:plc:ebt5msdpfavoklkap6gl54bm";
    let repo = IndexedRepo {
        did: did.to_string(),
        handle: Some("divine.test".to_string()),
        head: Some("head".to_string()),
        rev: Some("rev".to_string()),
        active: true,
        last_backfilled_at: None,
        last_seen_seq: None,
    };
    let indexed_at = Utc.with_ymd_and_hms(2026, 3, 21, 12, 0, 0).unwrap();
    let profile = IndexedProfile {
        did: did.to_string(),
        handle: Some("divine.test".to_string()),
        display_name: Some("Divine".to_string()),
        description: Some("Profile".to_string()),
        website: Some("https://divine.video".to_string()),
        avatar_cid: Some("bafkreiavatar".to_string()),
        banner_cid: None,
        created_at: Some(indexed_at),
        raw_json: Some("{\"website\":\"https://divine.video\"}".to_string()),
        indexed_at,
    };
    let posts = vec![
        fake_post_record(did, "MA6mjTWZKEB", "bafkreidemo001", indexed_at),
        fake_post_record(did, "hFxlUuKIIqU", "bafkreidemo002", indexed_at),
    ];
    let snapshot = RepoSnapshot {
        repo: repo.clone(),
        profile: Some(profile),
        posts: posts.clone(),
    };
    let client = FakePdsClient {
        repos: vec![repo],
        snapshots: HashMap::from([(did.to_string(), snapshot)]),
    };
    let store = MemoryStore::default();

    sync_repo_from_pds(&client, &store, did).await.unwrap();

    assert_eq!(store.posts().len(), 2);
    assert_eq!(
        store.posts()[0].embed_blob_cid.as_deref(),
        Some("bafkreidemo001")
    );
    assert_eq!(store.profiles().len(), 1);
}

fn fake_post_record(
    did: &str,
    rkey: &str,
    blob_cid: &str,
    indexed_at: chrono::DateTime<Utc>,
) -> IndexedPost {
    IndexedPost {
        uri: format!("at://{did}/app.bsky.feed.post/{rkey}"),
        did: did.to_string(),
        rkey: rkey.to_string(),
        record_cid: Some(format!("cid-{rkey}")),
        created_at: indexed_at,
        text: format!("post {rkey}"),
        langs_json: None,
        embed_blob_cid: Some(blob_cid.to_string()),
        embed_alt: None,
        aspect_ratio_width: Some(9),
        aspect_ratio_height: Some(16),
        raw_json: Some(format!("{{\"uri\":\"{rkey}\"}}")),
        search_text: format!("post {rkey}"),
        indexed_at,
        deleted_at: None,
    }
}
