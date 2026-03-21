use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::store::{IndexedPost, IndexedProfile, IndexedRepo};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSnapshot {
    pub repo: IndexedRepo,
    pub profile: Option<IndexedProfile>,
    pub posts: Vec<IndexedPost>,
}

#[async_trait]
pub trait PdsSource: Send + Sync {
    async fn list_repos(&self) -> Result<Vec<IndexedRepo>>;
    async fn sync_repo(&self, did: &str) -> Result<RepoSnapshot>;
}

#[derive(Debug, Clone)]
pub struct HttpPdsClient {
    base_url: String,
    client: reqwest::Client,
}

impl HttpPdsClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }

    async fn list_records(&self, did: &str, collection: &str) -> Result<Vec<RecordRow>> {
        let url = format!("{}/xrpc/com.atproto.repo.listRecords", self.base_url);
        let response = self
            .client
            .get(url)
            .query(&[("repo", did), ("collection", collection), ("limit", "100")])
            .send()
            .await
            .context("failed to call com.atproto.repo.listRecords")?;

        let status = response.status();
        anyhow::ensure!(
            status.is_success(),
            "listRecords failed with HTTP {}",
            status
        );

        let body: ListRecordsResponse = response
            .json()
            .await
            .context("failed to parse listRecords response")?;
        Ok(body.records)
    }

    fn parse_profile(
        repo: &IndexedRepo,
        record: &RecordRow,
        indexed_at: DateTime<Utc>,
    ) -> IndexedProfile {
        IndexedProfile {
            did: repo.did.clone(),
            handle: repo.handle.clone(),
            display_name: record
                .value
                .get("displayName")
                .and_then(Value::as_str)
                .map(str::to_string),
            description: record
                .value
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string),
            website: record
                .value
                .get("website")
                .and_then(Value::as_str)
                .map(str::to_string),
            avatar_cid: extract_blob_cid(record.value.get("avatar")),
            banner_cid: extract_blob_cid(record.value.get("banner")),
            created_at: record
                .value
                .get("createdAt")
                .and_then(parse_optional_datetime),
            raw_json: serde_json::to_string(&record.value).ok(),
            indexed_at,
        }
    }

    fn parse_post(
        repo: &IndexedRepo,
        record: &RecordRow,
        indexed_at: DateTime<Utc>,
    ) -> Result<IndexedPost> {
        let embed = record.value.get("embed");
        let aspect_ratio = embed.and_then(|value| value.get("aspectRatio"));
        let created_at = record
            .value
            .get("createdAt")
            .and_then(Value::as_str)
            .context("post record is missing createdAt")?;

        Ok(IndexedPost {
            uri: record.uri.clone(),
            did: repo.did.clone(),
            rkey: rkey_from_uri(&record.uri),
            record_cid: record.cid.clone(),
            created_at: DateTime::parse_from_rfc3339(created_at)
                .context("post createdAt is not valid RFC3339")?
                .with_timezone(&Utc),
            text: record
                .value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            langs_json: record
                .value
                .get("langs")
                .and_then(|value| serde_json::to_string(value).ok()),
            embed_blob_cid: embed
                .and_then(|value| value.get("video"))
                .and_then(|value| extract_blob_cid(Some(value))),
            embed_alt: embed
                .and_then(|value| value.get("alt"))
                .and_then(Value::as_str)
                .map(str::to_string),
            aspect_ratio_width: aspect_ratio
                .and_then(|value| value.get("width"))
                .and_then(Value::as_i64)
                .map(|value| value as i32),
            aspect_ratio_height: aspect_ratio
                .and_then(|value| value.get("height"))
                .and_then(Value::as_i64)
                .map(|value| value as i32),
            raw_json: serde_json::to_string(&record.value).ok(),
            search_text: record
                .value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            indexed_at,
            deleted_at: None,
        })
    }
}

#[async_trait]
impl PdsSource for HttpPdsClient {
    async fn list_repos(&self) -> Result<Vec<IndexedRepo>> {
        let url = format!("{}/xrpc/com.atproto.sync.listRepos", self.base_url);
        let response = self
            .client
            .get(url)
            .send()
            .await
            .context("failed to call com.atproto.sync.listRepos")?;

        let status = response.status();
        anyhow::ensure!(status.is_success(), "listRepos failed with HTTP {}", status);

        let body: ListReposResponse = response
            .json()
            .await
            .context("failed to parse listRepos response")?;

        Ok(body
            .repos
            .into_iter()
            .map(|repo| IndexedRepo {
                did: repo.did,
                handle: repo.handle,
                head: repo.head,
                rev: repo.rev,
                active: repo.active.unwrap_or(true),
                last_backfilled_at: None,
                last_seen_seq: None,
            })
            .collect())
    }

    async fn sync_repo(&self, did: &str) -> Result<RepoSnapshot> {
        let mut repo = self
            .list_repos()
            .await?
            .into_iter()
            .find(|entry| entry.did == did)
            .unwrap_or(IndexedRepo {
                did: did.to_string(),
                handle: None,
                head: None,
                rev: None,
                active: true,
                last_backfilled_at: None,
                last_seen_seq: None,
            });

        let indexed_at = Utc::now();
        repo.last_backfilled_at = Some(indexed_at);

        let profile = self
            .list_records(did, "app.bsky.actor.profile")
            .await?
            .into_iter()
            .next()
            .map(|record| Self::parse_profile(&repo, &record, indexed_at));

        let posts = self
            .list_records(did, "app.bsky.feed.post")
            .await?
            .into_iter()
            .map(|record| Self::parse_post(&repo, &record, indexed_at))
            .collect::<Result<Vec<_>>>()?;

        Ok(RepoSnapshot {
            repo,
            profile,
            posts,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ListReposResponse {
    repos: Vec<ListRepoEntry>,
}

#[derive(Debug, Deserialize)]
struct ListRepoEntry {
    did: String,
    handle: Option<String>,
    head: Option<String>,
    rev: Option<String>,
    active: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ListRecordsResponse {
    records: Vec<RecordRow>,
}

#[derive(Debug, Deserialize)]
struct RecordRow {
    uri: String,
    cid: Option<String>,
    value: Value,
}

fn extract_blob_cid(value: Option<&Value>) -> Option<String> {
    value
        .and_then(|entry| entry.get("ref"))
        .and_then(|entry| entry.get("$link"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn parse_optional_datetime(value: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn rkey_from_uri(uri: &str) -> String {
    uri.rsplit('/').next().unwrap_or_default().to_string()
}
