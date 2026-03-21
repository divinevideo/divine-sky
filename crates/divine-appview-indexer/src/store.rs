use std::sync::Mutex;

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use diesel::{Connection, PgConnection};
use divine_bridge_db::models::{
    NewAppviewMediaView, NewAppviewPost, NewAppviewProfile, NewAppviewRepo, NewAppviewServiceState,
};
use divine_bridge_db::{
    get_appview_profile_by_actor, get_appview_service_state, list_author_feed,
    list_latest_appview_posts, list_trending_appview_posts, load_post_with_media_view,
    search_appview_posts, upsert_appview_media_view, upsert_appview_post, upsert_appview_profile,
    upsert_appview_repo, upsert_appview_service_state,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedRepo {
    pub did: String,
    pub handle: Option<String>,
    pub head: Option<String>,
    pub rev: Option<String>,
    pub active: bool,
    pub last_backfilled_at: Option<DateTime<Utc>>,
    pub last_seen_seq: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedProfile {
    pub did: String,
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub website: Option<String>,
    pub avatar_cid: Option<String>,
    pub banner_cid: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub raw_json: Option<String>,
    pub indexed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedPost {
    pub uri: String,
    pub did: String,
    pub rkey: String,
    pub record_cid: Option<String>,
    pub created_at: DateTime<Utc>,
    pub text: String,
    pub langs_json: Option<String>,
    pub embed_blob_cid: Option<String>,
    pub embed_alt: Option<String>,
    pub aspect_ratio_width: Option<i32>,
    pub aspect_ratio_height: Option<i32>,
    pub raw_json: Option<String>,
    pub search_text: String,
    pub indexed_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaJob {
    pub did: String,
    pub blob_cid: String,
}

#[async_trait]
pub trait AppviewStore: Send + Sync {
    async fn upsert_repo(&self, repo: IndexedRepo) -> Result<()>;
    async fn upsert_profile(&self, profile: IndexedProfile) -> Result<()>;
    async fn upsert_post(&self, post: IndexedPost) -> Result<()>;
    async fn queue_media_job(&self, job: MediaJob) -> Result<()>;
    async fn set_service_state(&self, key: &str, value: Option<&str>) -> Result<()>;
}

#[derive(Default)]
pub struct MemoryStore {
    repos: Mutex<Vec<IndexedRepo>>,
    profiles: Mutex<Vec<IndexedProfile>>,
    posts: Mutex<Vec<IndexedPost>>,
    media_jobs: Mutex<Vec<MediaJob>>,
    service_state: Mutex<Vec<(String, Option<String>)>>,
}

impl MemoryStore {
    pub fn repos(&self) -> Vec<IndexedRepo> {
        self.repos.lock().unwrap().clone()
    }

    pub fn profiles(&self) -> Vec<IndexedProfile> {
        self.profiles.lock().unwrap().clone()
    }

    pub fn posts(&self) -> Vec<IndexedPost> {
        self.posts.lock().unwrap().clone()
    }

    pub fn media_jobs(&self) -> Vec<MediaJob> {
        self.media_jobs.lock().unwrap().clone()
    }
}

#[async_trait]
impl AppviewStore for MemoryStore {
    async fn upsert_repo(&self, repo: IndexedRepo) -> Result<()> {
        upsert_by_key(&self.repos, repo, |value| value.did.clone());
        Ok(())
    }

    async fn upsert_profile(&self, profile: IndexedProfile) -> Result<()> {
        upsert_by_key(&self.profiles, profile, |value| value.did.clone());
        Ok(())
    }

    async fn upsert_post(&self, post: IndexedPost) -> Result<()> {
        upsert_by_key(&self.posts, post, |value| value.uri.clone());
        Ok(())
    }

    async fn queue_media_job(&self, job: MediaJob) -> Result<()> {
        self.media_jobs.lock().unwrap().push(job);
        Ok(())
    }

    async fn set_service_state(&self, key: &str, value: Option<&str>) -> Result<()> {
        let mut state = self.service_state.lock().unwrap();
        if let Some(existing) = state.iter_mut().find(|entry| entry.0 == key) {
            existing.1 = value.map(str::to_string);
        } else {
            state.push((key.to_string(), value.map(str::to_string)));
        }
        Ok(())
    }
}

pub struct DbStore {
    database_url: String,
}

impl DbStore {
    pub fn new(database_url: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
        }
    }

    fn connect(&self) -> Result<PgConnection> {
        PgConnection::establish(&self.database_url).context("failed to connect to PostgreSQL")
    }
}

#[async_trait]
impl AppviewStore for DbStore {
    async fn upsert_repo(&self, repo: IndexedRepo) -> Result<()> {
        let mut conn = self.connect()?;
        upsert_appview_repo(
            &mut conn,
            &NewAppviewRepo {
                did: &repo.did,
                handle: repo.handle.as_deref(),
                head: repo.head.as_deref(),
                rev: repo.rev.as_deref(),
                active: repo.active,
                last_backfilled_at: repo.last_backfilled_at,
                last_seen_seq: repo.last_seen_seq,
            },
        )?;
        Ok(())
    }

    async fn upsert_profile(&self, profile: IndexedProfile) -> Result<()> {
        let mut conn = self.connect()?;
        upsert_appview_profile(
            &mut conn,
            &NewAppviewProfile {
                did: &profile.did,
                handle: profile.handle.as_deref(),
                display_name: profile.display_name.as_deref(),
                description: profile.description.as_deref(),
                website: profile.website.as_deref(),
                avatar_cid: profile.avatar_cid.as_deref(),
                banner_cid: profile.banner_cid.as_deref(),
                created_at: profile.created_at,
                raw_json: profile.raw_json.as_deref(),
                indexed_at: profile.indexed_at,
            },
        )?;
        Ok(())
    }

    async fn upsert_post(&self, post: IndexedPost) -> Result<()> {
        let mut conn = self.connect()?;
        upsert_appview_post(
            &mut conn,
            &NewAppviewPost {
                uri: &post.uri,
                did: &post.did,
                rkey: &post.rkey,
                record_cid: post.record_cid.as_deref(),
                created_at: post.created_at,
                text: &post.text,
                langs_json: post.langs_json.as_deref(),
                embed_blob_cid: post.embed_blob_cid.as_deref(),
                embed_alt: post.embed_alt.as_deref(),
                aspect_ratio_width: post.aspect_ratio_width,
                aspect_ratio_height: post.aspect_ratio_height,
                raw_json: post.raw_json.as_deref(),
                search_text: &post.search_text,
                indexed_at: post.indexed_at,
                deleted_at: post.deleted_at,
            },
        )?;
        Ok(())
    }

    async fn queue_media_job(&self, job: MediaJob) -> Result<()> {
        let mut conn = self.connect()?;
        upsert_appview_media_view(
            &mut conn,
            &NewAppviewMediaView {
                did: &job.did,
                blob_cid: &job.blob_cid,
                playlist_url: "",
                thumbnail_url: None,
                mime_type: "video/mp4",
                bytes: 0,
                ready: false,
                last_derived_at: None,
            },
        )?;
        Ok(())
    }

    async fn set_service_state(&self, key: &str, value: Option<&str>) -> Result<()> {
        let mut conn = self.connect()?;
        upsert_appview_service_state(
            &mut conn,
            &NewAppviewServiceState {
                state_key: key,
                state_value: value,
            },
        )?;
        Ok(())
    }
}

fn upsert_by_key<T, F>(storage: &Mutex<Vec<T>>, value: T, key_fn: F)
where
    T: Clone,
    F: Fn(&T) -> String,
{
    let mut guard = storage.lock().unwrap();
    let key = key_fn(&value);
    if let Some(existing) = guard.iter_mut().find(|entry| key_fn(entry) == key) {
        *existing = value;
    } else {
        guard.push(value);
    }
}

pub fn _query_smoke_symbols() {
    let _ = get_appview_profile_by_actor;
    let _ = get_appview_service_state;
    let _ = list_author_feed;
    let _ = list_latest_appview_posts;
    let _ = list_trending_appview_posts;
    let _ = search_appview_posts;
    let _ = load_post_with_media_view;
}
