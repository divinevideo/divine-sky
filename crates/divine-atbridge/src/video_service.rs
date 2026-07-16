//! Uploads video blobs through Bluesky's video transcoding service (video.bsky.app)
//! instead of direct PDS uploadBlob. This produces playable video embeds.
//!
//! Non-video MIME types are delegated to the inner PDS client.

use anyhow::{bail, Context, Result};
use divine_bridge_types::BlobRef;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

use crate::pipeline::BlobUploader;
use crate::publisher::{is_expired_token, PdsClient};

// ---------------------------------------------------------------------------
// Response types for the video service XRPC calls
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ServiceAuthResponse {
    token: String,
}

#[derive(Debug, Deserialize)]
struct UploadVideoResponse {
    #[serde(rename = "jobId")]
    job_id: Option<String>,
    #[allow(dead_code)]
    state: Option<String>,
    error: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JobStatusResponse {
    #[serde(rename = "jobStatus")]
    job_status: JobStatus,
}

#[derive(Debug, Deserialize)]
struct JobStatus {
    state: String,
    blob: Option<JobBlob>,
    error: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JobBlob {
    #[serde(rename = "ref")]
    blob_ref: JobBlobLink,
    #[serde(rename = "mimeType")]
    mime_type: String,
    size: u64,
}

#[derive(Debug, Deserialize)]
struct JobBlobLink {
    #[serde(rename = "$link")]
    link: String,
}

// ---------------------------------------------------------------------------
// VideoServiceUploader
// ---------------------------------------------------------------------------

/// Routes video uploads through `video.bsky.app` for transcoding and falls
/// back to direct PDS `uploadBlob` for non-video MIME types.
#[derive(Debug, Clone)]
pub struct VideoServiceUploader {
    /// HTTP client shared between video-service and PDS calls.
    client: Client,
    /// Underlying PDS client used for non-video blobs and as auth source.
    pds_client: PdsClient,
    /// PDS XRPC base URL (e.g. `https://pds.staging.dvines.org`).
    pds_url: String,
    /// Video transcoding service base URL (e.g. `https://video.bsky.app`).
    video_service_url: String,
    /// How long to poll `getJobStatus` before giving up.
    poll_timeout: Duration,
    /// Delay between successive `getJobStatus` polls.
    poll_interval: Duration,
}

impl VideoServiceUploader {
    pub fn new(
        pds_client: PdsClient,
        pds_url: String,
        video_service_url: String,
        poll_timeout: Duration,
        poll_interval: Duration,
    ) -> Self {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .build()
            .expect("failed to build reqwest client for video service");

        Self {
            client,
            pds_client,
            pds_url,
            video_service_url,
            poll_timeout,
            poll_interval,
        }
    }

    /// Obtain a service-auth token from the PDS.
    ///
    /// **Key gotchas (from real testing):**
    /// - `aud` must be the **PDS service DID** (not the video service DID)
    /// - `lxm` must be `com.atproto.repo.uploadBlob` (not `app.bsky.video.uploadVideo`)
    async fn get_service_auth(&self, user_did: &str) -> Result<String> {
        // Resolve the PDS service DID from the server description.
        let pds_service_did = self.resolve_pds_service_did().await?;

        let url = format!(
            "{}/xrpc/com.atproto.server.getServiceAuth?aud={}&lxm=com.atproto.repo.uploadBlob",
            self.pds_url, pds_service_did
        );

        // getServiceAuth issues a token for the *authenticated* account (rsky uses
        // AccessFull and sets iss = credentials.did), so it must be called as the
        // user's account session, not the shared admin token. On an expired token
        // (401, or 400 ExpiredToken) refresh the session once and retry.
        let mut auth_token = self.pds_client.auth_token_for(user_did).await?;
        let mut refreshed = false;
        loop {
            let resp = self
                .client
                .get(&url)
                .header("Authorization", format!("Bearer {auth_token}"))
                .send()
                .await
                .context("getServiceAuth request failed")?;

            let status = resp.status();
            let body = resp
                .text()
                .await
                .context("failed to read getServiceAuth response body")?;

            if !status.is_success() {
                if !refreshed && is_expired_token(status, &body) {
                    if let Some(new_token) = self.pds_client.refresh_session_for(user_did).await? {
                        auth_token = new_token;
                        refreshed = true;
                        continue;
                    }
                }
                bail!(
                    "getServiceAuth failed ({}) for did={}: {}",
                    status.as_u16(),
                    user_did,
                    body
                );
            }

            let auth: ServiceAuthResponse =
                serde_json::from_str(&body).context("failed to parse getServiceAuth response")?;
            return Ok(auth.token);
        }
    }

    /// Resolve the PDS service DID from `describeServer`.
    async fn resolve_pds_service_did(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct DescribeServer {
            did: String,
        }

        let url = format!("{}/xrpc/com.atproto.server.describeServer", self.pds_url);
        let resp: DescribeServer = self
            .client
            .get(&url)
            .send()
            .await
            .context("describeServer request failed")?
            .json()
            .await
            .context("describeServer parse failed")?;

        Ok(resp.did)
    }

    /// Upload the raw video bytes to the video service and return the job ID.
    async fn upload_to_video_service(
        &self,
        data: &[u8],
        service_token: &str,
        user_did: &str,
    ) -> Result<String> {
        let url = format!(
            "{}/xrpc/app.bsky.video.uploadVideo?did={}&name=divine-video.mp4",
            self.video_service_url, user_did
        );

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", service_token))
            .header("Content-Type", "video/mp4")
            .body(data.to_vec())
            .send()
            .await
            .context("uploadVideo request failed")?;

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();

        let upload: UploadVideoResponse = serde_json::from_str(&body_text).with_context(|| {
            format!(
                "uploadVideo response parse failed ({}): {}",
                status.as_u16(),
                body_text
            )
        })?;

        if !status.is_success() {
            bail!(
                "uploadVideo failed ({}): {}",
                status.as_u16(),
                upload.error.or(upload.message).unwrap_or(body_text)
            );
        }

        match upload.job_id {
            Some(id) => Ok(id),
            None => bail!(
                "uploadVideo returned no jobId: {}",
                upload
                    .error
                    .or(upload.message)
                    .unwrap_or_else(|| "unknown".to_string())
            ),
        }
    }

    /// Poll `getJobStatus` until the transcoding job completes or fails.
    async fn poll_job(&self, job_id: &str) -> Result<BlobRef> {
        let url = format!(
            "{}/xrpc/app.bsky.video.getJobStatus?jobId={}",
            self.video_service_url, job_id
        );
        let deadline = tokio::time::Instant::now() + self.poll_timeout;

        loop {
            if tokio::time::Instant::now() > deadline {
                bail!(
                    "video transcoding timed out after {:?} for job {}",
                    self.poll_timeout,
                    job_id
                );
            }

            tokio::time::sleep(self.poll_interval).await;

            let resp = self
                .client
                .get(&url)
                .send()
                .await
                .context("getJobStatus request failed")?;

            let status_code = resp.status();
            if !status_code.is_success() {
                let body = resp.text().await.unwrap_or_default();
                tracing::warn!(
                    "getJobStatus returned {} for job {}: {}; retrying",
                    status_code.as_u16(),
                    job_id,
                    body
                );
                continue;
            }

            let body: JobStatusResponse = resp.json().await.context("getJobStatus parse failed")?;

            match body.job_status.state.as_str() {
                "JOB_STATE_COMPLETED" => {
                    let blob = body
                        .job_status
                        .blob
                        .context("completed job has no blob ref")?;
                    return Ok(BlobRef::new(blob.blob_ref.link, blob.mime_type, blob.size));
                }
                "JOB_STATE_FAILED" => {
                    let detail = body
                        .job_status
                        .error
                        .or(body.job_status.message)
                        .unwrap_or_else(|| "unknown".to_string());
                    bail!("video transcoding failed for job {}: {}", job_id, detail);
                }
                other => {
                    tracing::debug!("video job {} state: {}", job_id, other);
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl BlobUploader for VideoServiceUploader {
    /// Non-video blobs are uploaded directly to the PDS.
    async fn upload_blob(&self, data: &[u8], mime_type: &str) -> Result<BlobRef> {
        self.pds_client.upload_blob(data, mime_type).await
    }

    /// Video blobs are routed through the video transcoding service.
    /// Non-video blobs fall through to the PDS.
    async fn upload_blob_for_user(
        &self,
        data: &[u8],
        mime_type: &str,
        user_did: &str,
    ) -> Result<BlobRef> {
        if !mime_type.starts_with("video/") {
            return self.pds_client.upload_blob(data, mime_type).await;
        }

        tracing::info!(
            did = %user_did,
            size = data.len(),
            "uploading video through video.bsky.app"
        );

        let service_token = self
            .get_service_auth(user_did)
            .await
            .context("failed to get service auth for video upload")?;

        let job_id = self
            .upload_to_video_service(data, &service_token, user_did)
            .await
            .context("failed to upload to video service")?;

        tracing::info!(
            job_id = %job_id,
            did = %user_did,
            "video upload job created, polling for completion"
        );

        self.poll_job(&job_id)
            .await
            .context("video transcoding job failed")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publisher::SessionProvider;
    use std::sync::Arc;

    fn test_uploader(pds_client: PdsClient, base_url: String) -> VideoServiceUploader {
        VideoServiceUploader::new(
            pds_client,
            base_url.clone(),
            base_url,
            Duration::from_secs(1),
            Duration::from_millis(1),
        )
    }

    /// Session provider that supplies a stale access token plus a refresh token,
    /// and records store_session calls, mirroring the publisher test provider.
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
    async fn get_service_auth_refreshes_session_and_retries_on_400_expired_token() {
        // getServiceAuth is called as the user's account session, so an expired
        // token (401 or 400 ExpiredToken) must refresh + retry.
        let mut server = mockito::Server::new_async().await;
        let describe = server
            .mock("GET", "/xrpc/com.atproto.server.describeServer")
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(serde_json::json!({"did": "did:web:pds.example"}).to_string())
            .create_async()
            .await;
        let expired = server
            .mock(
                "GET",
                "/xrpc/com.atproto.server.getServiceAuth?aud=did:web:pds.example&lxm=com.atproto.repo.uploadBlob",
            )
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
            .mock(
                "GET",
                "/xrpc/com.atproto.server.getServiceAuth?aud=did:web:pds.example&lxm=com.atproto.repo.uploadBlob",
            )
            .match_header("Authorization", "Bearer new-access-jwt")
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(serde_json::json!({"token": "service-token-123"}).to_string())
            .expect(1)
            .create_async()
            .await;

        let provider = Arc::new(RefreshingSessionProvider::default());
        let pds_client =
            PdsClient::new(server.url(), "shared").with_session_provider(provider.clone());
        let uploader = test_uploader(pds_client, server.url());

        let token = uploader
            .get_service_auth("did:plc:abc123")
            .await
            .expect("get_service_auth should refresh and succeed");

        assert_eq!(token, "service-token-123");
        describe.assert_async().await;
        expired.assert_async().await;
        refresh.assert_async().await;
        ok.assert_async().await;
        assert_eq!(
            provider.stored.lock().unwrap().as_slice(),
            &[("new-access-jwt".to_string(), "new-refresh-jwt".to_string())]
        );
    }

    #[tokio::test]
    async fn get_service_auth_bails_without_refresh_on_non_expired_error() {
        // A non-expired failure (e.g. 500) must NOT trigger a refresh; the error
        // propagates immediately.
        let mut server = mockito::Server::new_async().await;
        let describe = server
            .mock("GET", "/xrpc/com.atproto.server.describeServer")
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(serde_json::json!({"did": "did:web:pds.example"}).to_string())
            .create_async()
            .await;
        let failed = server
            .mock(
                "GET",
                "/xrpc/com.atproto.server.getServiceAuth?aud=did:web:pds.example&lxm=com.atproto.repo.uploadBlob",
            )
            .match_header("Authorization", "Bearer stale-access-jwt")
            .with_status(500)
            .with_body("boom")
            .expect(1)
            .create_async()
            .await;
        let refresh = server
            .mock("POST", "/xrpc/com.atproto.server.refreshSession")
            .expect(0)
            .create_async()
            .await;

        let provider = Arc::new(RefreshingSessionProvider::default());
        let pds_client =
            PdsClient::new(server.url(), "shared").with_session_provider(provider.clone());
        let uploader = test_uploader(pds_client, server.url());

        let err = uploader
            .get_service_auth("did:plc:abc123")
            .await
            .expect_err("500 should propagate without refresh");

        assert!(err.to_string().contains("500"), "expected 500 in: {err}");
        describe.assert_async().await;
        failed.assert_async().await;
        refresh.assert_async().await;
        assert!(provider.stored.lock().unwrap().is_empty());
    }

    #[test]
    fn upload_video_response_deserializes_with_job_id() {
        let json = r#"{"jobId":"job123","state":"JOB_STATE_CREATED"}"#;
        let resp: UploadVideoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.job_id.as_deref(), Some("job123"));
    }

    #[test]
    fn job_status_completed_deserializes() {
        let json = r#"{
            "jobStatus": {
                "state": "JOB_STATE_COMPLETED",
                "blob": {
                    "ref": {"$link": "bafkreivideo123"},
                    "mimeType": "video/mp4",
                    "size": 1024000
                }
            }
        }"#;
        let resp: JobStatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.job_status.state, "JOB_STATE_COMPLETED");
        let blob = resp.job_status.blob.unwrap();
        assert_eq!(blob.blob_ref.link, "bafkreivideo123");
        assert_eq!(blob.mime_type, "video/mp4");
        assert_eq!(blob.size, 1024000);
    }

    #[test]
    fn job_status_failed_deserializes() {
        let json = r#"{
            "jobStatus": {
                "state": "JOB_STATE_FAILED",
                "error": "encoding_error",
                "message": "unsupported codec"
            }
        }"#;
        let resp: JobStatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.job_status.state, "JOB_STATE_FAILED");
        assert_eq!(resp.job_status.error.as_deref(), Some("encoding_error"));
    }
}
