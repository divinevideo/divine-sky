//! ATProto XRPC client for PDS record operations.
//!
//! Provides `PdsClient` which wraps HTTP calls to the PDS XRPC endpoints
//! for creating, updating, and deleting records, as well as uploading blobs.

use anyhow::{Context, Result};
use divine_bridge_types::BlobRef;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use crate::pipeline::{BlobUploader, PdsPublisher, PublishedRecord};

/// Resolves the per-account PDS access token for repo writes. rsky-pds authorizes
/// `com.atproto.repo.*` writes as the repo's own DID, so each write must use that
/// account's session token rather than a shared admin token.
#[async_trait::async_trait]
pub trait SessionProvider: Send + Sync {
    /// Return the access JWT for `did`, or `None` to fall back to the default token.
    async fn access_token(&self, did: &str) -> Result<Option<String>>;

    /// Return the refresh JWT for `did`, used to mint a new access token on 401.
    async fn refresh_token(&self, did: &str) -> Result<Option<String>>;

    /// Persist a rotated session (access + refresh JWT) after a refresh.
    async fn store_session(&self, did: &str, access_jwt: &str, refresh_jwt: &str) -> Result<()>;
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Response from `com.atproto.repo.createRecord`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRecordResponse {
    pub uri: String,
    pub cid: String,
    #[serde(rename = "validationStatus")]
    pub validation_status: Option<String>,
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

#[derive(Debug, Deserialize)]
struct RefreshSessionResponse {
    #[serde(rename = "accessJwt")]
    access_jwt: String,
    #[serde(rename = "refreshJwt")]
    refresh_jwt: String,
}

// ---------------------------------------------------------------------------
// PdsClient
// ---------------------------------------------------------------------------

/// HTTP client for ATProto PDS XRPC endpoints.
#[derive(Clone)]
pub struct PdsClient {
    base_url: String,
    auth_token: String,
    session_provider: Option<Arc<dyn SessionProvider>>,
    client: reqwest::Client,
}

impl std::fmt::Debug for PdsClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdsClient")
            .field("base_url", &self.base_url)
            .field("has_session_provider", &self.session_provider.is_some())
            .finish_non_exhaustive()
    }
}

impl PdsClient {
    /// Create a new `PdsClient`.
    pub fn new(base_url: impl Into<String>, auth_token: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            auth_token: auth_token.into(),
            session_provider: None,
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(60))
                .build()
                .expect("reqwest client builder should succeed"),
        }
    }

    /// Attach a per-account session provider used to authenticate repo writes as
    /// the target DID. Falls back to the default token when the provider has no
    /// session for a DID.
    pub fn with_session_provider(mut self, provider: Arc<dyn SessionProvider>) -> Self {
        self.session_provider = Some(provider);
        self
    }

    /// Resolve the bearer token to use when acting as `did`'s account (its session
    /// token if known, else the default token). Used for repo writes and for
    /// `getServiceAuth`, both of which rsky authorizes as the account DID.
    pub async fn auth_token_for(&self, did: &str) -> Result<String> {
        if let Some(provider) = &self.session_provider {
            if let Some(token) = provider.access_token(did).await? {
                return Ok(token);
            }
        }
        Ok(self.auth_token.clone())
    }

    /// On an expired token for `did`, mint a fresh access token via `refreshSession`
    /// using the stored refresh JWT, persist the rotated session, and return the new
    /// access token. Returns `None` if there is no session provider or no refresh token.
    pub(crate) async fn refresh_session_for(&self, did: &str) -> Result<Option<String>> {
        let Some(provider) = &self.session_provider else {
            return Ok(None);
        };
        let Some(refresh_jwt) = provider.refresh_token(did).await? else {
            return Ok(None);
        };

        let url = format!("{}/xrpc/com.atproto.server.refreshSession", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {refresh_jwt}"))
            .send()
            .await
            .context("failed to send refreshSession request")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "refreshSession failed ({}): {}",
                status.as_u16(),
                parse_xrpc_error(&body)
            );
        }
        let refreshed: RefreshSessionResponse = resp
            .json()
            .await
            .context("failed to parse refreshSession response")?;
        provider
            .store_session(did, &refreshed.access_jwt, &refreshed.refresh_jwt)
            .await
            .context("failed to persist rotated PDS session")?;
        Ok(Some(refreshed.access_jwt))
    }

    /// POST a JSON repo-write to `path`, authenticated as `did`'s account, with a
    /// single refresh-and-retry on an expired token. Centralizes the per-account
    /// auth + token-refresh behavior for createRecord/putRecord/deleteRecord.
    ///
    /// Returns the final response status and its (already-consumed) body text. The
    /// body must be consumed here so we can inspect the XRPC error code to detect a
    /// `400 {"error":"ExpiredToken"}` expiry, so callers parse the returned body
    /// rather than the `reqwest::Response`.
    async fn post_repo_write_as(
        &self,
        did: &str,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<(reqwest::StatusCode, String)> {
        let url = format!("{}/xrpc/{path}", self.base_url);
        let mut auth_token = self.auth_token_for(did).await?;
        let mut refreshed = false;
        loop {
            let resp = self
                .client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {auth_token}"))
                .json(body)
                .send()
                .await
                .with_context(|| format!("failed to send {path} request"))?;

            let status = resp.status();
            let text = resp
                .text()
                .await
                .with_context(|| format!("failed to read {path} response body"))?;

            if !refreshed && is_expired_token(status, &text) {
                if let Some(new_token) = self.refresh_session_for(did).await? {
                    auth_token = new_token;
                    refreshed = true;
                    continue;
                }
            }
            return Ok((status, text));
        }
    }

    /// Upload a blob to the PDS using the shared static token.
    ///
    /// Calls `POST /xrpc/com.atproto.repo.uploadBlob` with raw bytes. The static
    /// token has no associated session/refresh JWT, so this path never refreshes.
    pub async fn upload_blob(&self, data: &[u8], mime_type: &str) -> Result<BlobRef> {
        self.upload_blob_inner(data, mime_type, self.auth_token.clone(), None)
            .await
    }

    /// Upload a blob authenticated as `did`'s account (per-account session token),
    /// with a single refresh-and-retry on an expired token.
    pub async fn upload_blob_for_did(
        &self,
        data: &[u8],
        mime_type: &str,
        did: &str,
    ) -> Result<BlobRef> {
        let token = self.auth_token_for(did).await?;
        self.upload_blob_inner(data, mime_type, token, Some(did))
            .await
    }

    /// Core uploadBlob request. When `did` is `Some`, an expired token triggers a
    /// single `refreshSession` + retry; when `None` (static token) it never refreshes.
    async fn upload_blob_inner(
        &self,
        data: &[u8],
        mime_type: &str,
        initial_token: String,
        did: Option<&str>,
    ) -> Result<BlobRef> {
        let url = format!("{}/xrpc/com.atproto.repo.uploadBlob", self.base_url);
        let mut auth_token = initial_token;
        let mut refreshed = false;
        loop {
            let resp = self
                .client
                .post(&url)
                .header("Content-Type", mime_type)
                .header("Authorization", format!("Bearer {auth_token}"))
                .body(data.to_vec())
                .send()
                .await
                .context("failed to send uploadBlob request")?;

            let status = resp.status();
            let text = resp
                .text()
                .await
                .context("failed to read uploadBlob response body")?;

            if !status.is_success() {
                if let Some(did) = did {
                    if !refreshed && is_expired_token(status, &text) {
                        if let Some(new_token) = self.refresh_session_for(did).await? {
                            auth_token = new_token;
                            refreshed = true;
                            continue;
                        }
                    }
                }
                let detail = parse_xrpc_error(&text);
                anyhow::bail!("uploadBlob failed ({}): {}", status.as_u16(), detail);
            }

            let upload: UploadBlobResponse =
                serde_json::from_str(&text).context("failed to parse uploadBlob response")?;

            let cid = upload.blob.ref_link.map(|r| r.link).unwrap_or_default();

            return Ok(BlobRef::new(cid, upload.blob.mime_type, upload.blob.size));
        }
    }

    /// Create a record in a PDS repository.
    ///
    /// Calls `POST /xrpc/com.atproto.repo.createRecord`.
    pub async fn create_record(
        &self,
        did: &str,
        collection: &str,
        record: &serde_json::Value,
    ) -> Result<CreateRecordResponse> {
        let body = serde_json::json!({
            "repo": did,
            "collection": collection,
            "validate": true,
            "record": record,
        });

        let (status, resp_body) = self
            .post_repo_write_as(did, "com.atproto.repo.createRecord", &body)
            .await?;

        if !status.is_success() {
            let detail = parse_xrpc_error(&resp_body);
            anyhow::bail!("createRecord failed ({}): {}", status.as_u16(), detail);
        }

        let response: CreateRecordResponse =
            serde_json::from_str(&resp_body).context("failed to parse createRecord response")?;

        if matches!(response.validation_status.as_deref(), Some("unknown")) {
            anyhow::bail!("createRecord returned unknown validation status");
        }

        Ok(response)
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
        let body = serde_json::json!({
            "repo": did,
            "collection": collection,
            "rkey": rkey,
            "record": record,
        });

        let (status, resp_body) = self
            .post_repo_write_as(did, "com.atproto.repo.putRecord", &body)
            .await?;

        if !status.is_success() {
            let detail = parse_xrpc_error(&resp_body);
            anyhow::bail!("putRecord failed ({}): {}", status.as_u16(), detail);
        }

        serde_json::from_str(&resp_body).context("failed to parse putRecord response")
    }

    /// Delete a record from a PDS repository.
    ///
    /// Calls `POST /xrpc/com.atproto.repo.deleteRecord`.
    pub async fn delete_record(&self, did: &str, collection: &str, rkey: &str) -> Result<()> {
        let body = serde_json::json!({
            "repo": did,
            "collection": collection,
            "rkey": rkey,
        });

        let (status, resp_body) = self
            .post_repo_write_as(did, "com.atproto.repo.deleteRecord", &body)
            .await?;

        if !status.is_success() {
            let detail = parse_xrpc_error(&resp_body);
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

    async fn upload_blob_for_user(
        &self,
        data: &[u8],
        mime_type: &str,
        user_did: &str,
    ) -> Result<BlobRef> {
        PdsClient::upload_blob_for_did(self, data, mime_type, user_did).await
    }
}

#[async_trait::async_trait]
impl PdsPublisher for PdsClient {
    async fn create_record(
        &self,
        did: &str,
        collection: &str,
        record: &serde_json::Value,
    ) -> Result<String> {
        Ok(PdsClient::create_record(self, did, collection, record)
            .await?
            .uri)
    }

    async fn create_record_with_meta(
        &self,
        did: &str,
        collection: &str,
        record: &serde_json::Value,
    ) -> Result<PublishedRecord> {
        let response = PdsClient::create_record(self, did, collection, record).await?;
        let rkey = response
            .uri
            .rsplit('/')
            .next()
            .filter(|segment| !segment.is_empty())
            .context("createRecord response URI is missing an rkey segment")?
            .to_string();

        Ok(PublishedRecord {
            at_uri: response.uri,
            rkey,
            cid: Some(response.cid),
        })
    }

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
            rkey: rkey.to_string(),
            cid: Some(response.cid),
        })
    }

    async fn delete_record(&self, did: &str, collection: &str, rkey: &str) -> Result<()> {
        PdsClient::delete_record(self, did, collection, rkey).await
    }
}

/// Detect whether an XRPC response indicates an expired access token, matching the
/// `@atproto/api` reference client: a token is expired when the status is `401`, or
/// when the status is `400` and the XRPC error code is exactly `ExpiredToken`.
///
/// This is deliberately narrow: a `400 {"error":"InvalidRequest"}` (or any other
/// non-`ExpiredToken` 400) is a genuine bad request and must NOT trigger a refresh,
/// to avoid masking real errors or looping. Centralized so every per-account-token
/// call site (repo writes, uploadBlob, getServiceAuth) shares one predicate.
pub(crate) fn is_expired_token(status: reqwest::StatusCode, body: &str) -> bool {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return true;
    }
    if status == reqwest::StatusCode::BAD_REQUEST {
        if let Ok(err) = serde_json::from_str::<XrpcError>(body) {
            return err.error.as_deref() == Some("ExpiredToken");
        }
    }
    false
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
    use reqwest::StatusCode;

    #[test]
    fn is_expired_token_matches_reference_client_semantics() {
        // 401 -> expired regardless of body.
        assert!(is_expired_token(StatusCode::UNAUTHORIZED, ""));
        assert!(is_expired_token(
            StatusCode::UNAUTHORIZED,
            r#"{"error":"AuthenticationRequired"}"#
        ));
        // 400 with ExpiredToken -> expired.
        assert!(is_expired_token(
            StatusCode::BAD_REQUEST,
            r#"{"error":"ExpiredToken","message":"Token is expired"}"#
        ));
        // 400 with a different error code -> NOT expired.
        assert!(!is_expired_token(
            StatusCode::BAD_REQUEST,
            r#"{"error":"InvalidRequest"}"#
        ));
        // 400 with an unparseable body -> NOT expired.
        assert!(!is_expired_token(StatusCode::BAD_REQUEST, "not json"));
        // 400 with no error field -> NOT expired.
        assert!(!is_expired_token(
            StatusCode::BAD_REQUEST,
            r#"{"message":"x"}"#
        ));
        // Other statuses -> NOT expired.
        assert!(!is_expired_token(StatusCode::INTERNAL_SERVER_ERROR, ""));
        assert!(!is_expired_token(StatusCode::OK, ""));
    }

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
                    "validate": true,
                    "record": {"text": "hello"}
                })
                .to_string(),
            ))
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                serde_json::json!({
                    "uri": "at://did:plc:abc123/app.bsky.feed.post/my-rkey",
                    "cid": "bafyrei123",
                    "validationStatus": "valid"
                })
                .to_string(),
            )
            .create_async()
            .await;

        let client = PdsClient::new(server.url(), "test-token");
        let record = serde_json::json!({"text": "hello"});
        let resp = client
            .create_record("did:plc:abc123", "app.bsky.feed.post", &record)
            .await
            .unwrap();

        assert_eq!(resp.uri, "at://did:plc:abc123/app.bsky.feed.post/my-rkey");
        assert_eq!(resp.cid, "bafyrei123");
        assert_eq!(resp.validation_status.as_deref(), Some("valid"));
        mock.assert_async().await;
    }

    #[derive(Debug)]
    struct StaticSessionProvider {
        token: String,
    }

    #[async_trait::async_trait]
    impl SessionProvider for StaticSessionProvider {
        async fn access_token(&self, _did: &str) -> Result<Option<String>> {
            Ok(Some(self.token.clone()))
        }
        async fn refresh_token(&self, _did: &str) -> Result<Option<String>> {
            Ok(None)
        }
        async fn store_session(&self, _did: &str, _access: &str, _refresh: &str) -> Result<()> {
            Ok(())
        }
    }

    /// Records refresh + store calls so tests can assert the 401-refresh-retry flow.
    #[derive(Default)]
    struct RefreshingSessionProvider {
        stored: std::sync::Mutex<Vec<(String, String)>>,
    }

    #[async_trait::async_trait]
    impl SessionProvider for RefreshingSessionProvider {
        async fn access_token(&self, _did: &str) -> Result<Option<String>> {
            Ok(Some("stale-access-jwt".to_string()))
        }
        async fn refresh_token(&self, _did: &str) -> Result<Option<String>> {
            Ok(Some("refresh-jwt".to_string()))
        }
        async fn store_session(&self, _did: &str, access: &str, refresh: &str) -> Result<()> {
            self.stored
                .lock()
                .unwrap()
                .push((access.to_string(), refresh.to_string()));
            Ok(())
        }
    }

    #[tokio::test]
    async fn create_record_refreshes_session_and_retries_on_401() {
        let mut server = mockito::Server::new_async().await;
        // First createRecord with the stale token -> 401.
        let unauthorized = server
            .mock("POST", "/xrpc/com.atproto.repo.createRecord")
            .match_header("Authorization", "Bearer stale-access-jwt")
            .with_status(401)
            .with_body(serde_json::json!({"error": "ExpiredToken"}).to_string())
            .expect(1)
            .create_async()
            .await;
        // refreshSession returns rotated tokens.
        let refresh = server
            .mock("POST", "/xrpc/com.atproto.server.refreshSession")
            .match_header("Authorization", "Bearer refresh-jwt")
            .with_status(200)
            .with_body(
                serde_json::json!({"accessJwt": "new-access-jwt", "refreshJwt": "new-refresh-jwt"})
                    .to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        // Retry with the new token -> success.
        let ok = server
            .mock("POST", "/xrpc/com.atproto.repo.createRecord")
            .match_header("Authorization", "Bearer new-access-jwt")
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                serde_json::json!({
                    "uri": "at://did:plc:abc123/app.bsky.feed.post/rk",
                    "cid": "bafyrei123",
                    "validationStatus": "valid"
                })
                .to_string(),
            )
            .expect(1)
            .create_async()
            .await;

        let provider = Arc::new(RefreshingSessionProvider::default());
        let client = PdsClient::new(server.url(), "shared").with_session_provider(provider.clone());
        client
            .create_record(
                "did:plc:abc123",
                "app.bsky.feed.post",
                &serde_json::json!({"text": "hi"}),
            )
            .await
            .unwrap();

        unauthorized.assert_async().await;
        refresh.assert_async().await;
        ok.assert_async().await;
        // The rotated session was persisted.
        assert_eq!(
            provider.stored.lock().unwrap().as_slice(),
            &[("new-access-jwt".to_string(), "new-refresh-jwt".to_string())]
        );
    }

    #[tokio::test]
    async fn create_record_refreshes_session_and_retries_on_400_expired_token() {
        // Reference @atproto/api treats 400 {"error":"ExpiredToken"} as an expired
        // token, exactly like a 401. createRecord must refresh + retry on it too.
        let mut server = mockito::Server::new_async().await;
        let expired = server
            .mock("POST", "/xrpc/com.atproto.repo.createRecord")
            .match_header("Authorization", "Bearer stale-access-jwt")
            .with_status(400)
            .with_body(
                serde_json::json!({"error": "ExpiredToken", "message": "Token is expired"})
                    .to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        let refresh = server
            .mock("POST", "/xrpc/com.atproto.server.refreshSession")
            .match_header("Authorization", "Bearer refresh-jwt")
            .with_status(200)
            .with_body(
                serde_json::json!({"accessJwt": "new-access-jwt", "refreshJwt": "new-refresh-jwt"})
                    .to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        let ok = server
            .mock("POST", "/xrpc/com.atproto.repo.createRecord")
            .match_header("Authorization", "Bearer new-access-jwt")
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                serde_json::json!({
                    "uri": "at://did:plc:abc123/app.bsky.feed.post/rk",
                    "cid": "bafyrei123",
                    "validationStatus": "valid"
                })
                .to_string(),
            )
            .expect(1)
            .create_async()
            .await;

        let provider = Arc::new(RefreshingSessionProvider::default());
        let client = PdsClient::new(server.url(), "shared").with_session_provider(provider.clone());
        client
            .create_record(
                "did:plc:abc123",
                "app.bsky.feed.post",
                &serde_json::json!({"text": "hi"}),
            )
            .await
            .unwrap();

        expired.assert_async().await;
        refresh.assert_async().await;
        ok.assert_async().await;
        assert_eq!(
            provider.stored.lock().unwrap().as_slice(),
            &[("new-access-jwt".to_string(), "new-refresh-jwt".to_string())]
        );
    }

    #[tokio::test]
    async fn create_record_does_not_refresh_on_400_invalid_request() {
        // A genuine 400 (bad request) must NOT trigger a refresh/retry loop —
        // refreshing would mask the real error and could loop. The error must
        // propagate and refreshSession must never be called.
        let mut server = mockito::Server::new_async().await;
        let bad = server
            .mock("POST", "/xrpc/com.atproto.repo.createRecord")
            .match_header("Authorization", "Bearer stale-access-jwt")
            .with_status(400)
            .with_body(
                serde_json::json!({"error": "InvalidRequest", "message": "bad record"}).to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        // If a refresh is (wrongly) attempted, this mock would be hit; expect(0).
        let refresh = server
            .mock("POST", "/xrpc/com.atproto.server.refreshSession")
            .expect(0)
            .create_async()
            .await;

        let provider = Arc::new(RefreshingSessionProvider::default());
        let client = PdsClient::new(server.url(), "shared").with_session_provider(provider.clone());
        let err = client
            .create_record(
                "did:plc:abc123",
                "app.bsky.feed.post",
                &serde_json::json!({"text": "hi"}),
            )
            .await
            .expect_err("400 InvalidRequest should propagate as an error");

        let msg = err.to_string();
        assert!(msg.contains("400"), "expected 400 in: {msg}");
        assert!(
            msg.contains("InvalidRequest"),
            "expected error name in: {msg}"
        );
        bad.assert_async().await;
        refresh.assert_async().await;
        assert!(
            provider.stored.lock().unwrap().is_empty(),
            "no session should have been stored"
        );
    }

    #[tokio::test]
    async fn upload_blob_for_did_refreshes_session_and_retries_on_400_expired_token() {
        // Per-account blob uploads must also refresh + retry on an expired token.
        let mut server = mockito::Server::new_async().await;
        let expired = server
            .mock("POST", "/xrpc/com.atproto.repo.uploadBlob")
            .match_header("Authorization", "Bearer stale-access-jwt")
            .with_status(400)
            .with_body(
                serde_json::json!({"error": "ExpiredToken", "message": "Token is expired"})
                    .to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        let refresh = server
            .mock("POST", "/xrpc/com.atproto.server.refreshSession")
            .match_header("Authorization", "Bearer refresh-jwt")
            .with_status(200)
            .with_body(
                serde_json::json!({"accessJwt": "new-access-jwt", "refreshJwt": "new-refresh-jwt"})
                    .to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        let ok = server
            .mock("POST", "/xrpc/com.atproto.repo.uploadBlob")
            .match_header("Authorization", "Bearer new-access-jwt")
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                serde_json::json!({
                    "blob": {
                        "ref": {"$link": "bafblob"},
                        "mimeType": "video/mp4",
                        "size": 3
                    }
                })
                .to_string(),
            )
            .expect(1)
            .create_async()
            .await;

        let provider = Arc::new(RefreshingSessionProvider::default());
        let client = PdsClient::new(server.url(), "shared").with_session_provider(provider.clone());
        let blob = client
            .upload_blob_for_did(b"abc", "video/mp4", "did:plc:abc123")
            .await
            .unwrap();

        assert_eq!(blob.cid(), "bafblob");
        expired.assert_async().await;
        refresh.assert_async().await;
        ok.assert_async().await;
        assert_eq!(
            provider.stored.lock().unwrap().as_slice(),
            &[("new-access-jwt".to_string(), "new-refresh-jwt".to_string())]
        );
    }

    #[tokio::test]
    async fn upload_blob_static_token_does_not_refresh_on_expired_token() {
        // The static-token upload_blob has no per-account session; it must NOT
        // attempt a refresh even if the PDS returns an expired-token error.
        let mut server = mockito::Server::new_async().await;
        let expired = server
            .mock("POST", "/xrpc/com.atproto.repo.uploadBlob")
            .with_status(400)
            .with_body(serde_json::json!({"error": "ExpiredToken"}).to_string())
            .expect(1)
            .create_async()
            .await;
        let refresh = server
            .mock("POST", "/xrpc/com.atproto.server.refreshSession")
            .expect(0)
            .create_async()
            .await;

        let provider = Arc::new(RefreshingSessionProvider::default());
        let client = PdsClient::new(server.url(), "shared").with_session_provider(provider.clone());
        let err = client
            .upload_blob(b"abc", "video/mp4")
            .await
            .expect_err("static-token upload should not refresh");

        assert!(err.to_string().contains("ExpiredToken"));
        expired.assert_async().await;
        refresh.assert_async().await;
    }

    #[tokio::test]
    async fn create_record_uses_per_account_session_token() {
        // rsky authorizes repo writes as the account DID, so create_record must
        // send the account's session token (from the provider), not the shared one.
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/xrpc/com.atproto.repo.createRecord")
            .match_header("Authorization", "Bearer account-session-jwt")
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                serde_json::json!({
                    "uri": "at://did:plc:abc123/app.bsky.feed.post/rk",
                    "cid": "bafyrei123",
                    "validationStatus": "valid"
                })
                .to_string(),
            )
            .create_async()
            .await;

        let client = PdsClient::new(server.url(), "shared-admin-token").with_session_provider(
            Arc::new(StaticSessionProvider {
                token: "account-session-jwt".to_string(),
            }),
        );
        client
            .create_record(
                "did:plc:abc123",
                "app.bsky.feed.post",
                &serde_json::json!({"text": "hi"}),
            )
            .await
            .unwrap();
        mock.assert_async().await; // fails if it sent the shared token instead
    }

    #[tokio::test]
    async fn upload_blob_for_user_uses_per_account_session_token() {
        // Video crossposts upload the blob as the account; it must use the
        // account's session token, not the shared one.
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/xrpc/com.atproto.repo.uploadBlob")
            .match_header("Authorization", "Bearer account-session-jwt")
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                serde_json::json!({
                    "blob": {
                        "ref": {"$link": "bafblob"},
                        "mimeType": "video/mp4",
                        "size": 3
                    }
                })
                .to_string(),
            )
            .create_async()
            .await;

        let client = PdsClient::new(server.url(), "shared-admin-token").with_session_provider(
            Arc::new(StaticSessionProvider {
                token: "account-session-jwt".to_string(),
            }),
        );
        client
            .upload_blob_for_did(b"abc", "video/mp4", "did:plc:abc123")
            .await
            .unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn create_record_rejects_unknown_validation_status() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/xrpc/com.atproto.repo.createRecord")
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                serde_json::json!({
                    "uri": "at://did:plc:abc123/app.bsky.feed.post/my-rkey",
                    "cid": "bafyrei123",
                    "validationStatus": "unknown"
                })
                .to_string(),
            )
            .create_async()
            .await;

        let client = PdsClient::new(server.url(), "test-token");
        let record = serde_json::json!({"text": "hello"});
        let err = client
            .create_record("did:plc:abc123", "app.bsky.feed.post", &record)
            .await
            .expect_err("unknown validation status should fail");

        assert!(err.to_string().contains("unknown validation status"));
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
    async fn put_record_returns_error_on_non_success() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/xrpc/com.atproto.repo.putRecord")
            .with_status(400)
            .with_header("Content-Type", "application/json")
            .with_body(
                serde_json::json!({"error": "InvalidRequest", "message": "bad rkey"}).to_string(),
            )
            .create_async()
            .await;

        let client = PdsClient::new(server.url(), "tok");
        let err = client
            .put_record("did:plc:x", "col", "rk", &serde_json::json!({}))
            .await
            .expect_err("putRecord non-success should error");

        let msg = err.to_string();
        assert!(msg.contains("400"), "expected 400 in: {msg}");
        assert!(
            msg.contains("InvalidRequest"),
            "expected error name in: {msg}"
        );
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
    async fn delete_record_uses_per_account_session_token() {
        // Deletes (e.g. tombstoning a crossposted video) must also auth as the account.
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/xrpc/com.atproto.repo.deleteRecord")
            .match_header("Authorization", "Bearer account-session-jwt")
            .with_status(200)
            .with_body("{}")
            .create_async()
            .await;

        let client = PdsClient::new(server.url(), "shared").with_session_provider(Arc::new(
            StaticSessionProvider {
                token: "account-session-jwt".to_string(),
            },
        ));
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
            .create_record("did:plc:x", "col", &serde_json::json!({}))
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
            .create_record("did:plc:x", "col", &serde_json::json!({}))
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
