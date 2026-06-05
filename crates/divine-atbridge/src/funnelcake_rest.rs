//! Funnelcake REST client for live video-event ingest.
//!
//! Replaces the WebSocket firehose for live ingest: poll
//! `GET {base}/videos/events?kind=&sort=recent&limit=&before=` and feed the
//! returned full Nostr events into the same publish pipeline. The endpoint
//! returns video kinds (34235/34236); deletions (kind 5) are handled separately.

use std::time::Duration;

use anyhow::{Context, Result};
use divine_bridge_types::NostrEvent;
use serde::Deserialize;

/// One page of video events from `GET /videos/events`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoEventsPage {
    /// The full Nostr events on this page (newest first when `sort=recent`).
    pub events: Vec<NostrEvent>,
    /// Cursor (unix seconds) to pass as `before` for the next older page.
    pub next_cursor: Option<i64>,
    /// Whether more (older) pages exist.
    pub has_more: bool,
}

#[derive(Debug, Clone)]
pub struct FunnelcakeRestClient {
    /// REST API base, e.g. `https://relay.staging.dvines.org/api`.
    base_url: String,
    client: reqwest::Client,
}

impl FunnelcakeRestClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .build()
            .expect("reqwest client builder should succeed");
        Self {
            base_url: base_url.into(),
            client,
        }
    }

    fn video_events_endpoint(&self) -> String {
        format!("{}/videos/events", self.base_url.trim_end_matches('/'))
    }

    /// Fetch one page of video events for `kind`, newest first. `before` is a
    /// unix-second cursor (exclusive upper bound) for backward pagination.
    pub async fn fetch_video_events(
        &self,
        kind: u64,
        before: Option<i64>,
        limit: u32,
    ) -> Result<VideoEventsPage> {
        let kind = kind.to_string();
        let limit = limit.to_string();
        let mut query: Vec<(&str, String)> = vec![
            ("sort", "recent".to_string()),
            ("kind", kind),
            ("limit", limit),
        ];
        if let Some(before) = before {
            query.push(("before", before.to_string()));
        }

        let response = self
            .client
            .get(self.video_events_endpoint())
            .query(&query)
            .send()
            .await
            .context("sending funnelcake videos/events request")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("videos/events failed ({}): {body}", status.as_u16());
        }

        let raw: RawVideoEventsResponse = response
            .json()
            .await
            .context("parsing funnelcake videos/events response")?;

        Ok(VideoEventsPage {
            events: raw.videos.into_iter().map(|v| v.event).collect(),
            next_cursor: raw.next_cursor.map(|c| c.0),
            has_more: raw.has_more,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawVideoEventsResponse {
    #[serde(default)]
    videos: Vec<RawVideoEntry>,
    #[serde(default)]
    next_cursor: Option<FlexInt>,
    #[serde(default)]
    has_more: bool,
}

#[derive(Debug, Deserialize)]
struct RawVideoEntry {
    event: NostrEvent,
}

/// `next_cursor` arrives as either a JSON number or a stringified number
/// depending on the endpoint version; accept both.
#[derive(Debug, Clone, Copy)]
struct FlexInt(i64);

impl<'de> Deserialize<'de> for FlexInt {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::Number(n) => n
                .as_i64()
                .map(FlexInt)
                .ok_or_else(|| D::Error::custom("next_cursor number is not an i64")),
            serde_json::Value::String(s) => s
                .trim()
                .parse::<i64>()
                .map(FlexInt)
                .map_err(|_| D::Error::custom("next_cursor string is not an integer")),
            other => Err(D::Error::custom(format!(
                "next_cursor must be a number or string, got {other}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fetch_video_events_parses_events_and_cursor() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/videos/events")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("kind".into(), "34236".into()),
                mockito::Matcher::UrlEncoded("sort".into(), "recent".into()),
                mockito::Matcher::UrlEncoded("limit".into(), "50".into()),
                mockito::Matcher::UrlEncoded("before".into(), "1700000100".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "videos": [
                        { "event": {
                            "id": "a".repeat(64),
                            "pubkey": "b".repeat(64),
                            "created_at": 1700000050i64,
                            "kind": 34236,
                            "tags": [["d", "vid1"]],
                            "content": "hi",
                            "sig": "c".repeat(128)
                        }},
                        { "event": {
                            "id": "d".repeat(64),
                            "pubkey": "e".repeat(64),
                            "created_at": 1700000010i64,
                            "kind": 34236,
                            "tags": [],
                            "content": "yo",
                            "sig": "f".repeat(128)
                        }}
                    ],
                    "next_cursor": "1700000000",
                    "has_more": true
                })
                .to_string(),
            )
            .create_async()
            .await;

        let client = FunnelcakeRestClient::new(format!("{}/", server.url()));
        let page = client
            .fetch_video_events(34236, Some(1700000100), 50)
            .await
            .expect("page should parse");

        assert_eq!(page.events.len(), 2);
        assert_eq!(page.events[0].id, "a".repeat(64));
        assert_eq!(page.events[0].created_at, 1700000050);
        assert_eq!(page.next_cursor, Some(1700000000));
        assert!(page.has_more);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn fetch_video_events_accepts_numeric_cursor_and_missing_fields() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/videos/events")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            // numeric next_cursor (live endpoint shape), no `before` sent
            .with_body(
                serde_json::json!({
                    "videos": [],
                    "next_cursor": 1699999000i64,
                    "has_more": false
                })
                .to_string(),
            )
            .create_async()
            .await;

        let client = FunnelcakeRestClient::new(server.url());
        let page = client
            .fetch_video_events(34235, None, 100)
            .await
            .expect("page should parse");
        assert!(page.events.is_empty());
        assert_eq!(page.next_cursor, Some(1699999000));
        assert!(!page.has_more);
    }
}
