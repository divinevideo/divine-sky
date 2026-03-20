//! ATProto XRPC client for PDS record operations.
//!
//! Provides `PdsClient` which wraps HTTP calls to the PDS XRPC endpoints
//! for creating, updating, and deleting records, as well as uploading blobs.

use anyhow::{Context, Result};
use divine_bridge_types::BlobRef;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::pipeline::{BlobUploader, PdsPublisher, PublishedRecord};

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Response from `com.atproto.repo.createRecord`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRecordResponse {
    pub uri: String,
    pub cid: String,
}

/// Response from `com.atproto.repo.putRecord`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutRecordResponse {
    pub uri: String,
    pub cid: String,
}

/// The blob object returned by `com.atproto.repo.uploadBlob`.
#[derive(Debug, Clone, Deserialize)]
struct UploadBlobResponse {
    blob: UploadedBlob,
}

#[derive(Debug, Clone, Deserialize)]
struct UploadedBlob {
    #[serde(rename = "$type")]
    _type: Option<String>,
    #[serde(rename = "ref")]
    ref_link: Option<BlobLink>,
    #[serde(rename = "mimeType")]
    mime_type: String,
    size: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct BlobLink {
    #[serde(rename = "$link")]
    link: String,
}

/// XRPC error response body.
#[derive(Debug, Clone, Deserialize)]
struct XrpcError {
    error: Option<String>,
    message: Option<String>,
}

// ---------------------------------------------------------------------------
// PdsClient
// ---------------------------------------------------------------------------

/// HTTP client for ATProto PDS XRPC endpoints.
#[derive(Debug, Clone)]
pub struct PdsClient {
    base_url: String,
    auth_token: String,
    client: reqwest::Client,
}

impl PdsClient {
    /// Create a new `PdsClient`.
    pub fn new(base_url: impl Into<String>, auth_token: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            auth_token: auth_token.into(),
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(60))
                .build()
                .expect("reqwest client builder should succeed"),
        }
    }

    /// Upload a blob to the PDS.
    ///
    /// Calls `POST /xrpc/com.atproto.repo.uploadBlob` with raw bytes.
    pub async fn upload_blob(&self, data: &[u8], mime_type: &str) -> Result<BlobRef> {
        let url = format!("{}/xrpc/com.atproto.repo.uploadBlob", self.base_url);

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", mime_type)
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .body(data.to_vec())
            .send()
            .await
            .context("failed to send uploadBlob request")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let detail = parse_xrpc_error(&body);
            anyhow::bail!("uploadBlob failed ({}): {}", status.as_u16(), detail);
        }

        let upload: UploadBlobResponse = resp
            .json()
            .await
            .context("failed to parse uploadBlob response")?;

        let cid = upload.blob.ref_link.map(|r| r.link).unwrap_or_default();

        Ok(BlobRef::new(cid, upload.blob.mime_type, upload.blob.size))
    }

    /// Create a record in a PDS repository.
    ///
    /// Calls `POST /xrpc/com.atproto.repo.createRecord`.
    pub async fn create_record(
        &self,
        did: &str,
        collection: &str,
        rkey: &str,
        record: &serde_json::Value,
    ) -> Result<CreateRecordResponse> {
        let url = format!("{}/xrpc/com.atproto.repo.createRecord", self.base_url);

        let body = serde_json::json!({
            "repo": did,
            "collection": collection,
            "rkey": rkey,
            "record": record,
        });

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .json(&body)
            .send()
            .await
            .context("failed to send createRecord request")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let detail = parse_xrpc_error(&body);
            anyhow::bail!("createRecord failed ({}): {}", status.as_u16(), detail);
        }

        resp.json()
            .await
            .context("failed to parse createRecord response")
    }

    /// Upsert a record in a PDS repository.
    ///
    /// Calls `POST /xrpc/com.atproto.repo.putRecord`.
    pub async fn put_record(
        &self,
        did: &str,
        collection: &str,
        rkey: &str,
        record: &serde_json::Value,
    ) -> Result<PutRecordResponse> {
        let url = format!("{}/xrpc/com.atproto.repo.putRecord", self.base_url);

        let body = serde_json::json!({
            "repo": did,
            "collection": collection,
            "rkey": rkey,
            "record": record,
        });

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .json(&body)
            .send()
            .await
            .context("failed to send putRecord request")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let detail = parse_xrpc_error(&body);
            anyhow::bail!("putRecord failed ({}): {}", status.as_u16(), detail);
        }

        resp.json()
            .await
            .context("failed to parse putRecord response")
    }

    /// Delete a record from a PDS repository.
    ///
    /// Calls `POST /xrpc/com.atproto.repo.deleteRecord`.
    pub async fn delete_record(&self, did: &str, collection: &str, rkey: &str) -> Result<()> {
        let url = format!("{}/xrpc/com.atproto.repo.deleteRecord", self.base_url);

        let body = serde_json::json!({
            "repo": did,
            "collection": collection,
            "rkey": rkey,
        });

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .json(&body)
            .send()
            .await
            .context("failed to send deleteRecord request")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let detail = parse_xrpc_error(&body);
            anyhow::bail!("deleteRecord failed ({}): {}", status.as_u16(), detail);
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl BlobUploader for PdsClient {
    async fn upload_blob(&self, data: &[u8], mime_type: &str) -> Result<BlobRef> {
        PdsClient::upload_blob(self, data, mime_type).await
    }
}

#[async_trait::async_trait]
impl PdsPublisher for PdsClient {
    async fn put_record(
        &self,
        did: &str,
        collection: &str,
        rkey: &str,
        record: &serde_json::Value,
    ) -> Result<String> {
        Ok(PdsClient::put_record(self, did, collection, rkey, record)
            .await?
            .uri)
    }

    async fn put_record_with_meta(
        &self,
        did: &str,
        collection: &str,
        rkey: &str,
        record: &serde_json::Value,
    ) -> Result<PublishedRecord> {
        let response = PdsClient::put_record(self, did, collection, rkey, record).await?;
        Ok(PublishedRecord {
            at_uri: response.uri,
            cid: Some(response.cid),
        })
    }

    async fn delete_record(&self, did: &str, collection: &str, rkey: &str) -> Result<()> {
        PdsClient::delete_record(self, did, collection, rkey).await
    }
}

/// Try to extract a human-readable message from an XRPC error response body.
fn parse_xrpc_error(body: &str) -> String {
    if let Ok(err) = serde_json::from_str::<XrpcError>(body) {
        let error = err.error.unwrap_or_default();
        let message = err.message.unwrap_or_default();
        if !error.is_empty() && !message.is_empty() {
            return format!("{}: {}", error, message);
        }
        if !error.is_empty() {
            return error;
        }
        if !message.is_empty() {
            return message;
        }
    }
    body.to_string()
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_record_sends_correct_body_and_parses_response() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/xrpc/com.atproto.repo.createRecord")
            .match_header("Authorization", "Bearer test-token")
            .match_header("Content-Type", "application/json")
            .match_body(mockito::Matcher::JsonString(
                serde_json::json!({
                    "repo": "did:plc:abc123",
                    "collection": "app.bsky.feed.post",
                    "rkey": "my-rkey",
                    "record": {"text": "hello"}
                })
                .to_string(),
            ))
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                serde_json::json!({
                    "uri": "at://did:plc:abc123/app.bsky.feed.post/my-rkey",
                    "cid": "bafyrei123"
                })
                .to_string(),
            )
            .create_async()
            .await;

        let client = PdsClient::new(server.url(), "test-token");
        let record = serde_json::json!({"text": "hello"});
        let resp = client
            .create_record("did:plc:abc123", "app.bsky.feed.post", "my-rkey", &record)
            .await
            .unwrap();

        assert_eq!(resp.uri, "at://did:plc:abc123/app.bsky.feed.post/my-rkey");
        assert_eq!(resp.cid, "bafyrei123");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn put_record_sends_correct_body() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/xrpc/com.atproto.repo.putRecord")
            .match_header("Authorization", "Bearer tok")
            .match_body(mockito::Matcher::JsonString(
                serde_json::json!({
                    "repo": "did:plc:xyz",
                    "collection": "app.bsky.feed.post",
                    "rkey": "rk1",
                    "record": {"text": "updated"}
                })
                .to_string(),
            ))
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                serde_json::json!({
                    "uri": "at://did:plc:xyz/app.bsky.feed.post/rk1",
                    "cid": "bafyrei456"
                })
                .to_string(),
            )
            .create_async()
            .await;

        let client = PdsClient::new(server.url(), "tok");
        let record = serde_json::json!({"text": "updated"});
        let resp = client
            .put_record("did:plc:xyz", "app.bsky.feed.post", "rk1", &record)
            .await
            .unwrap();

        assert_eq!(resp.uri, "at://did:plc:xyz/app.bsky.feed.post/rk1");
        assert_eq!(resp.cid, "bafyrei456");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn delete_record_sends_correct_request() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/xrpc/com.atproto.repo.deleteRecord")
            .match_header("Authorization", "Bearer tok")
            .match_body(mockito::Matcher::JsonString(
                serde_json::json!({
                    "repo": "did:plc:xyz",
                    "collection": "app.bsky.feed.post",
                    "rkey": "rk1"
                })
                .to_string(),
            ))
            .with_status(200)
            .with_body("{}")
            .create_async()
            .await;

        let client = PdsClient::new(server.url(), "tok");
        client
            .delete_record("did:plc:xyz", "app.bsky.feed.post", "rk1")
            .await
            .unwrap();

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn upload_blob_sends_raw_bytes_with_correct_content_type() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/xrpc/com.atproto.repo.uploadBlob")
            .match_header("Content-Type", "video/mp4")
            .match_header("Authorization", "Bearer tok")
            .match_body(vec![0xDEu8, 0xAD, 0xBE, 0xEF])
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                serde_json::json!({
                    "blob": {
                        "$type": "blob",
                        "ref": {"$link": "bafyreiblob123"},
                        "mimeType": "video/mp4",
                        "size": 4
                    }
                })
                .to_string(),
            )
            .create_async()
            .await;

        let client = PdsClient::new(server.url(), "tok");
        let blob = client
            .upload_blob(&[0xDE, 0xAD, 0xBE, 0xEF], "video/mp4")
            .await
            .unwrap();

        assert_eq!(blob.cid(), "bafyreiblob123");
        assert_eq!(blob.mime_type, "video/mp4");
        assert_eq!(blob.size, 4);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn http_400_returns_error_with_detail() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/xrpc/com.atproto.repo.createRecord")
            .with_status(400)
            .with_header("Content-Type", "application/json")
            .with_body(
                serde_json::json!({
                    "error": "InvalidRequest",
                    "message": "Record is missing required field"
                })
                .to_string(),
            )
            .create_async()
            .await;

        let client = PdsClient::new(server.url(), "tok");
        let err = client
            .create_record("did:plc:x", "col", "rk", &serde_json::json!({}))
            .await
            .unwrap_err();

        let msg = err.to_string();
        assert!(msg.contains("400"), "expected 400 in: {}", msg);
        assert!(
            msg.contains("InvalidRequest"),
            "expected error name in: {}",
            msg
        );
    }

    #[tokio::test]
    async fn http_401_returns_auth_error() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/xrpc/com.atproto.repo.createRecord")
            .with_status(401)
            .with_header("Content-Type", "application/json")
            .with_body(
                serde_json::json!({
                    "error": "AuthenticationRequired",
                    "message": "Invalid token"
                })
                .to_string(),
            )
            .create_async()
            .await;

        let client = PdsClient::new(server.url(), "bad-tok");
        let err = client
            .create_record("did:plc:x", "col", "rk", &serde_json::json!({}))
            .await
            .unwrap_err();

        let msg = err.to_string();
        assert!(msg.contains("401"), "expected 401 in: {}", msg);
    }

    #[tokio::test]
    async fn http_500_returns_server_error() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/xrpc/com.atproto.repo.deleteRecord")
            .with_status(500)
            .with_body("Internal Server Error")
            .create_async()
            .await;

        let client = PdsClient::new(server.url(), "tok");
        let err = client
            .delete_record("did:plc:x", "col", "rk")
            .await
            .unwrap_err();

        let msg = err.to_string();
        assert!(msg.contains("500"), "expected 500 in: {}", msg);
    }
}
