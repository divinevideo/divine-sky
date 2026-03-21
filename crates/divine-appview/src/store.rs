use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use diesel::{Connection, PgConnection};
use divine_bridge_db::{
    get_appview_profile_by_actor, get_appview_service_state, list_author_feed,
    load_post_with_media_view, search_appview_posts,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProfile {
    pub did: String,
    pub handle: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub avatar: Option<String>,
    pub banner: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPost {
    pub uri: String,
    pub cid: Option<String>,
    pub did: String,
    pub handle: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub avatar: Option<String>,
    pub banner: Option<String>,
    pub text: String,
    pub created_at: DateTime<Utc>,
    pub embed_blob_cid: Option<String>,
    pub embed_alt: Option<String>,
    pub playlist_url: Option<String>,
    pub thumbnail_url: Option<String>,
}

#[async_trait]
pub trait AppviewStore: Send + Sync {
    async fn get_profile(&self, actor: &str) -> Result<Option<StoredProfile>>;
    async fn get_posts(&self, uris: &[String]) -> Result<Vec<StoredPost>>;
    async fn get_post(&self, uri: &str) -> Result<Option<StoredPost>>;
    async fn get_author_feed(
        &self,
        actor: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<StoredPost>>;
    async fn search_posts(
        &self,
        query: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<StoredPost>>;
    async fn readiness(&self) -> Result<bool>;
}

pub type DynStore = Arc<dyn AppviewStore>;

pub struct DbStore {
    database_url: String,
    media_base_url: String,
}

impl DbStore {
    pub fn new(database_url: impl Into<String>, media_base_url: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
            media_base_url: media_base_url.into(),
        }
    }

    fn connect(&self) -> Result<PgConnection> {
        PgConnection::establish(&self.database_url).context("failed to connect to PostgreSQL")
    }

    fn media_blob_url(&self, did: &str, cid: &str) -> String {
        let did_path = did.replace(':', "/");
        format!(
            "{}/blobs/{did_path}/{cid}",
            self.media_base_url.trim_end_matches('/')
        )
    }

    fn map_profile_row(&self, row: divine_bridge_db::models::AppviewProfile) -> StoredProfile {
        StoredProfile {
            did: row.did.clone(),
            handle: row.handle.unwrap_or(row.did.clone()),
            display_name: row.display_name,
            description: row.description,
            avatar: row
                .avatar_cid
                .as_deref()
                .map(|cid| self.media_blob_url(&row.did, cid)),
            banner: row
                .banner_cid
                .as_deref()
                .map(|cid| self.media_blob_url(&row.did, cid)),
        }
    }

    fn load_post_internal(&self, conn: &mut PgConnection, uri: &str) -> Result<Option<StoredPost>> {
        let Some(row) = load_post_with_media_view(conn, uri)? else {
            return Ok(None);
        };
        let Some(profile_row) = get_appview_profile_by_actor(conn, &row.did)? else {
            return Ok(None);
        };
        let profile = self.map_profile_row(profile_row);

        Ok(Some(StoredPost {
            uri: row.uri,
            cid: row.record_cid,
            did: profile.did.clone(),
            handle: profile.handle,
            display_name: profile.display_name,
            description: profile.description,
            avatar: profile.avatar,
            banner: profile.banner,
            text: row.text,
            created_at: row.created_at,
            embed_blob_cid: row.embed_blob_cid.clone(),
            embed_alt: row.embed_alt,
            playlist_url: row.playlist_url.filter(|value| !value.is_empty()),
            thumbnail_url: row.thumbnail_url,
        }))
    }
}

#[async_trait]
impl AppviewStore for DbStore {
    async fn get_profile(&self, actor: &str) -> Result<Option<StoredProfile>> {
        let mut conn = self.connect()?;
        Ok(get_appview_profile_by_actor(&mut conn, actor)?.map(|row| self.map_profile_row(row)))
    }

    async fn get_posts(&self, uris: &[String]) -> Result<Vec<StoredPost>> {
        let mut conn = self.connect()?;
        let mut posts = Vec::new();
        for uri in uris {
            if let Some(post) = self.load_post_internal(&mut conn, uri)? {
                posts.push(post);
            }
        }
        Ok(posts)
    }

    async fn get_post(&self, uri: &str) -> Result<Option<StoredPost>> {
        let mut conn = self.connect()?;
        self.load_post_internal(&mut conn, uri)
    }

    async fn get_author_feed(
        &self,
        actor: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<StoredPost>> {
        let mut conn = self.connect()?;
        let parsed_cursor = cursor
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc));
        let rows = list_author_feed(&mut conn, actor, limit as i64, parsed_cursor)?;
        let mut posts = Vec::new();
        for row in rows {
            if let Some(post) = self.load_post_internal(&mut conn, &row.uri)? {
                posts.push(post);
            }
        }
        Ok(posts)
    }

    async fn search_posts(
        &self,
        query: &str,
        limit: usize,
        _cursor: Option<String>,
    ) -> Result<Vec<StoredPost>> {
        let mut conn = self.connect()?;
        let rows = search_appview_posts(&mut conn, query, limit as i64)?;
        let mut posts = Vec::new();
        for row in rows {
            if let Some(post) = self.load_post_internal(&mut conn, &row.uri)? {
                posts.push(post);
            }
        }
        Ok(posts)
    }

    async fn readiness(&self) -> Result<bool> {
        let mut conn = self.connect()?;
        Ok(get_appview_service_state(&mut conn, "appview_last_backfill")?.is_some())
    }
}
