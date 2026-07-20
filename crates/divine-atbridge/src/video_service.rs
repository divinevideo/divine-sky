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
use crate::publisher::PdsClient;

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
    blob: Option<JobBlob>,
    error: Option<String>,
    message: Option<String>,
}

#[derive(Debug)]
enum VideoUploadOutcome {
    FreshJob(String),
    CachedJob(String),
    FreshBlob(BlobRef),
    CachedBlob(BlobRef),
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

/// Lifetime requested for the service-auth token. video.bsky.app holds the
/// token across the transcode and then uses it to upload the finished blob
/// back to the PDS, so the PDS default of 60s is too short.
const SERVICE_AUTH_TOKEN_TTL_SECS: i64 = 1800;

/// Unix-epoch expiry for a fresh service-auth token (now + 30 minutes).
fn service_auth_exp_epoch() -> i64 {
    chrono::Utc::now().timestamp() + SERVICE_AUTH_TOKEN_TTL_SECS
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
            "{}/xrpc/com.atproto.server.getServiceAuth?aud={}&lxm=com.atproto.repo.uploadBlob&exp={}",
            self.pds_url,
            pds_service_did,
            service_auth_exp_epoch()
        );

        // getServiceAuth issues a token for the *authenticated* account (rsky uses
        // AccessFull and sets iss = credentials.did), so it must be called as the
        // user's account session, not the shared admin token.
        let auth_token = self.pds_client.auth_token_for(user_did).await?;
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {auth_token}"))
            .send()
            .await
            .context("getServiceAuth request failed")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!(
                "getServiceAuth failed ({}) for did={}: {}",
                status.as_u16(),
                user_did,
                body
            );
        }

        let auth: ServiceAuthResponse = resp
            .json()
            .await
            .context("failed to parse getServiceAuth response")?;
        Ok(auth.token)
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

    /// Upload the raw video bytes and retain whether the response came from the
    /// service's cross-account processing cache. Blob CIDs are content-addressed,
    /// but their availability is scoped to a PDS repository.
    async fn upload_to_video_service(
        &self,
        data: &[u8],
        service_token: &str,
        user_did: &str,
    ) -> Result<VideoUploadOutcome> {
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

        let cached = status == reqwest::StatusCode::CONFLICT
            || upload.error.as_deref() == Some("already_exists");

        if let Some(blob) = upload.blob {
            let blob = BlobRef::new(blob.blob_ref.link, blob.mime_type, blob.size);
            return Ok(if cached {
                VideoUploadOutcome::CachedBlob(blob)
            } else {
                VideoUploadOutcome::FreshBlob(blob)
            });
        }

        if !status.is_success() {
            // A 409 without a blob can still carry the existing job ID; reuse
            // it — polling getJobStatus resolves a completed cached job to its
            // blob ref. (A 409 with a blob was already handled above.)
            if status == reqwest::StatusCode::CONFLICT {
                if let Some(id) = upload.job_id.as_deref().filter(|id| !id.is_empty()) {
                    tracing::info!(
                        job_id = %id,
                        did = %user_did,
                        "video already has a transcoding job; reusing it"
                    );
                    return Ok(VideoUploadOutcome::CachedJob(id.to_string()));
                }
            }
            bail!(
                "uploadVideo failed ({}): {}",
                status.as_u16(),
                upload.error.or(upload.message).unwrap_or(body_text)
            );
        }

        match upload.job_id {
            Some(id) if cached => Ok(VideoUploadOutcome::CachedJob(id)),
            Some(id) => Ok(VideoUploadOutcome::FreshJob(id)),
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
            // Non-video blobs (captions, avatars, banners) still go straight to
            // the PDS — but as the ACCOUNT, not the shared admin token: rsky
            // authorizes repo writes per-DID and rejects the admin token with
            // `BadJwt` (it isn't a JWT at all).
            return self
                .pds_client
                .upload_blob_for_did(data, mime_type, user_did)
                .await;
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

        let upload = self
            .upload_to_video_service(data, &service_token, user_did)
            .await
            .context("failed to upload to video service")?;

        let (job_id, cached) = match upload {
            VideoUploadOutcome::CachedBlob(blob) => {
                tracing::info!(
                    did = %user_did,
                    cached_cid = %blob.cid(),
                    "video cache hit is not proof of target-repository blob ownership"
                );
                return self
                    .pds_client
                    .upload_blob_for_did(data, mime_type, user_did)
                    .await
                    .context("failed to upload cached video source to target PDS");
            }
            VideoUploadOutcome::FreshBlob(blob) => return Ok(blob),
            VideoUploadOutcome::FreshJob(job_id) => (job_id, false),
            VideoUploadOutcome::CachedJob(job_id) => (job_id, true),
        };

        tracing::info!(
            job_id = %job_id,
            did = %user_did,
            "video upload job created, polling for completion"
        );

        let transcoded = self
            .poll_job(&job_id)
            .await
            .context("video transcoding job failed")?;

        if cached {
            tracing::info!(
                job_id = %job_id,
                did = %user_did,
                cached_cid = %transcoded.cid(),
                "cached video job completed; uploading source into target repository"
            );
            return self
                .pds_client
                .upload_blob_for_did(data, mime_type, user_did)
                .await
                .context("failed to upload cached video source to target PDS");
        }

        Ok(transcoded)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Matcher;

    #[tokio::test]
    async fn cached_video_is_uploaded_directly_to_target_pds() {
        let mut server = mockito::Server::new_async().await;
        let describe = server
            .mock("GET", "/xrpc/com.atproto.server.describeServer")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"did":"did:web:pds.example"}"#)
            .create_async()
            .await;
        let service_auth = server
            .mock("GET", "/xrpc/com.atproto.server.getServiceAuth")
            .match_header("authorization", "Bearer account-token")
            .match_query(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"token":"service-token"}"#)
            .create_async()
            .await;
        let upload = server
            .mock("POST", "/xrpc/app.bsky.video.uploadVideo")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("did".into(), "did:plc:alice".into()),
                Matcher::UrlEncoded("name".into(), "divine-video.mp4".into()),
            ]))
            .with_status(409)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "jobId": "existing-job",
                    "state": "JOB_STATE_FAILED",
                    "error": "already_exists",
                    "message": "video has already been processed",
                    "blob": {
                        "$type": "blob",
                        "ref": { "$link": "bafkreiotherrepo" },
                        "mimeType": "video/mp4",
                        "size": 1_866_809
                    }
                })
                .to_string(),
            )
            .create_async()
            .await;
        let target_upload = server
            .mock("POST", "/xrpc/com.atproto.repo.uploadBlob")
            .match_header("authorization", "Bearer account-token")
            .match_header("content-type", "video/mp4")
            .match_body(b"video".to_vec())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "blob": {
                        "$type": "blob",
                        "ref": { "$link": "bafkreitargetrepo" },
                        "mimeType": "video/mp4",
                        "size": 5
                    }
                })
                .to_string(),
            )
            .create_async()
            .await;

        let uploader = VideoServiceUploader::new(
            PdsClient::new(server.url(), "account-token"),
            server.url(),
            server.url(),
            Duration::from_secs(1),
            Duration::ZERO,
        );

        let blob = uploader
            .upload_blob_for_user(b"video", "video/mp4", "did:plc:alice")
            .await
            .expect("cached video should be uploaded into the target repository");

        assert_eq!(blob.cid(), "bafkreitargetrepo");
        assert_ne!(blob.cid(), "bafkreiotherrepo");
        assert_eq!(blob.mime_type, "video/mp4");
        assert_eq!(blob.size, 5);
        describe.assert_async().await;
        service_auth.assert_async().await;
        upload.assert_async().await;
        target_upload.assert_async().await;
    }

    #[tokio::test]
    async fn upload_video_returns_job_for_new_video() {
        let mut server = mockito::Server::new_async().await;
        let upload = server
            .mock("POST", "/xrpc/app.bsky.video.uploadVideo")
            .match_query(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"jobId":"new-job","state":"JOB_STATE_CREATED"}"#)
            .create_async()
            .await;
        let uploader = test_uploader(server.url());

        let outcome = uploader
            .upload_to_video_service(b"video", "service-token", "did:plc:alice")
            .await
            .expect("a new video should return its job");

        assert!(matches!(outcome, VideoUploadOutcome::FreshJob(id) if id == "new-job"));
        upload.assert_async().await;
    }

    #[tokio::test]
    async fn upload_video_returns_fresh_blob_for_new_completed_video() {
        let mut server = mockito::Server::new_async().await;
        let upload = server
            .mock("POST", "/xrpc/app.bsky.video.uploadVideo")
            .match_query(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "state": "JOB_STATE_COMPLETED",
                    "blob": {
                        "$type": "blob",
                        "ref": { "$link": "bafkreifreshrepo" },
                        "mimeType": "video/mp4",
                        "size": 42
                    }
                })
                .to_string(),
            )
            .create_async()
            .await;
        let uploader = test_uploader(server.url());

        let outcome = uploader
            .upload_to_video_service(b"video", "service-token", "did:plc:alice")
            .await
            .expect("a completed fresh upload should return its blob");

        assert!(matches!(
            outcome,
            VideoUploadOutcome::FreshBlob(blob)
                if blob.cid() == "bafkreifreshrepo" && blob.size == 42
        ));
        upload.assert_async().await;
    }

    #[tokio::test]
    async fn successful_already_exists_response_is_still_a_cached_job() {
        let mut server = mockito::Server::new_async().await;
        let upload = server
            .mock("POST", "/xrpc/app.bsky.video.uploadVideo")
            .match_query(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"jobId":"existing-job","state":"JOB_STATE_COMPLETED","error":"already_exists"}"#,
            )
            .create_async()
            .await;
        let uploader = test_uploader(server.url());

        let outcome = uploader
            .upload_to_video_service(b"video", "service-token", "did:plc:alice")
            .await
            .expect("already_exists should preserve cache provenance at any HTTP status");

        assert!(matches!(outcome, VideoUploadOutcome::CachedJob(id) if id == "existing-job"));
        upload.assert_async().await;
    }

    #[tokio::test]
    async fn upload_video_rejects_already_exists_without_reusable_blob() {
        let mut server = mockito::Server::new_async().await;
        let upload = server
            .mock("POST", "/xrpc/app.bsky.video.uploadVideo")
            .match_query(Matcher::Any)
            .with_status(409)
            .with_header("content-type", "application/json")
            .with_body(r#"{"error":"already_exists","message":"blob unavailable"}"#)
            .create_async()
            .await;
        let uploader = test_uploader(server.url());

        let error = uploader
            .upload_to_video_service(b"video", "service-token", "did:plc:alice")
            .await
            .expect_err("already_exists without a blob is not reusable");

        assert!(error.to_string().contains("already_exists"));
        upload.assert_async().await;
    }

    fn test_uploader(video_service_url: String) -> VideoServiceUploader {
        VideoServiceUploader::new(
            PdsClient::new("http://127.0.0.1:9", "unused"),
            "http://127.0.0.1:9".to_string(),
            video_service_url,
            Duration::from_secs(1),
            Duration::ZERO,
        )
    }

    #[test]
    fn upload_video_response_deserializes_with_job_id() {
        let json = r#"{"jobId":"job123","state":"JOB_STATE_CREATED"}"#;
        let resp: UploadVideoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.job_id.as_deref(), Some("job123"));
    }

    #[test]
    fn service_auth_exp_is_thirty_minutes_out() {
        let now = chrono::Utc::now().timestamp();
        let exp = service_auth_exp_epoch();
        assert!(
            (exp - now - 1800).abs() <= 5,
            "exp should be ~now+1800s, got now+{}s",
            exp - now
        );
    }

    #[test]
    fn upload_video_response_deserializes_409_already_exists() {
        let json = r#"{"jobId":"jobX","error":"already_exists","state":"JOB_STATE_COMPLETED","did":"did:plc:user"}"#;
        let resp: UploadVideoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.job_id.as_deref(), Some("jobX"));
        assert_eq!(resp.error.as_deref(), Some("already_exists"));
    }

    #[tokio::test]
    async fn upload_409_already_exists_with_job_id_resolves_via_job_status() {
        let mut server = mockito::Server::new_async().await;

        let describe = server
            .mock("GET", "/xrpc/com.atproto.server.describeServer")
            .with_status(200)
            .with_body(r#"{"did":"did:web:pds.example"}"#)
            .create_async()
            .await;
        // The service-auth token must request a 30-minute expiry: video.bsky.app
        // holds it across the transcode and uses it to upload the blob back to
        // the PDS, so the default 60s would 401 the callback.
        let service_auth = server
            .mock("GET", "/xrpc/com.atproto.server.getServiceAuth")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("lxm".into(), "com.atproto.repo.uploadBlob".into()),
                mockito::Matcher::Regex(r"exp=\d{10}".into()),
            ]))
            .with_status(200)
            .with_body(r#"{"token":"service-token"}"#)
            .create_async()
            .await;
        let upload = server
            .mock("POST", "/xrpc/app.bsky.video.uploadVideo")
            .match_query(mockito::Matcher::Any)
            .with_status(409)
            .with_body(
                r#"{"jobId":"jobX","error":"already_exists","state":"JOB_STATE_COMPLETED","did":"did:plc:user"}"#,
            )
            .create_async()
            .await;
        let job_status = server
            .mock("GET", "/xrpc/app.bsky.video.getJobStatus")
            .match_query(mockito::Matcher::UrlEncoded("jobId".into(), "jobX".into()))
            .with_status(200)
            .with_body(
                r#"{
                    "jobStatus": {
                        "state": "JOB_STATE_COMPLETED",
                        "blob": {
                            "ref": {"$link": "bafkreicachedvideo"},
                            "mimeType": "video/mp4",
                            "size": 2048
                        }
                    }
                }"#,
            )
            .create_async()
            .await;
        let target_upload = server
            .mock("POST", "/xrpc/com.atproto.repo.uploadBlob")
            .match_header("authorization", "Bearer tok")
            .match_header("content-type", "video/mp4")
            .match_body(b"video-bytes".to_vec())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "blob": {
                        "$type": "blob",
                        "ref": { "$link": "bafkreitargetvideo" },
                        "mimeType": "video/mp4",
                        "size": 11
                    }
                })
                .to_string(),
            )
            .create_async()
            .await;

        let uploader = VideoServiceUploader::new(
            PdsClient::new(server.url(), "tok"),
            server.url(),
            server.url(),
            Duration::from_secs(5),
            Duration::from_millis(10),
        );

        let blob = uploader
            .upload_blob_for_user(b"video-bytes", "video/mp4", "did:plc:user")
            .await
            .expect("a cached job should upload source bytes into the target repository");

        assert_eq!(blob.cid(), "bafkreitargetvideo");
        assert_eq!(blob.mime_type, "video/mp4");
        assert_eq!(blob.size, 11);
        describe.assert_async().await;
        service_auth.assert_async().await;
        upload.assert_async().await;
        job_status.assert_async().await;
        target_upload.assert_async().await;
    }

    #[tokio::test]
    async fn fresh_video_job_does_not_call_direct_pds_upload() {
        let mut server = mockito::Server::new_async().await;
        let describe = server
            .mock("GET", "/xrpc/com.atproto.server.describeServer")
            .with_status(200)
            .with_body(r#"{"did":"did:web:pds.example"}"#)
            .create_async()
            .await;
        let service_auth = server
            .mock("GET", "/xrpc/com.atproto.server.getServiceAuth")
            .match_query(Matcher::Any)
            .with_status(200)
            .with_body(r#"{"token":"service-token"}"#)
            .create_async()
            .await;
        let upload = server
            .mock("POST", "/xrpc/app.bsky.video.uploadVideo")
            .match_query(Matcher::Any)
            .with_status(200)
            .with_body(r#"{"jobId":"fresh-job","state":"JOB_STATE_CREATED"}"#)
            .create_async()
            .await;
        let job_status = server
            .mock("GET", "/xrpc/app.bsky.video.getJobStatus")
            .match_query(Matcher::UrlEncoded("jobId".into(), "fresh-job".into()))
            .with_status(200)
            .with_body(
                r#"{
                    "jobStatus": {
                        "state": "JOB_STATE_COMPLETED",
                        "blob": {
                            "ref": {"$link": "bafkreifreshvideo"},
                            "mimeType": "video/mp4",
                            "size": 4096
                        }
                    }
                }"#,
            )
            .create_async()
            .await;

        let uploader = VideoServiceUploader::new(
            PdsClient::new(server.url(), "tok"),
            server.url(),
            server.url(),
            Duration::from_secs(5),
            Duration::ZERO,
        );

        let blob = uploader
            .upload_blob_for_user(b"fresh-video", "video/mp4", "did:plc:user")
            .await
            .expect("fresh video job should use its target-repository result");

        assert_eq!(blob.cid(), "bafkreifreshvideo");
        assert_eq!(blob.size, 4096);
        describe.assert_async().await;
        service_auth.assert_async().await;
        upload.assert_async().await;
        job_status.assert_async().await;
    }

    #[tokio::test]
    async fn upload_409_without_job_id_fails() {
        let mut server = mockito::Server::new_async().await;

        server
            .mock("GET", "/xrpc/com.atproto.server.describeServer")
            .with_status(200)
            .with_body(r#"{"did":"did:web:pds.example"}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/xrpc/com.atproto.server.getServiceAuth")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body(r#"{"token":"service-token"}"#)
            .create_async()
            .await;
        server
            .mock("POST", "/xrpc/app.bsky.video.uploadVideo")
            .match_query(mockito::Matcher::Any)
            .with_status(409)
            .with_body(r#"{"error":"already_exists","message":"video already exists"}"#)
            .create_async()
            .await;

        let uploader = VideoServiceUploader::new(
            PdsClient::new(server.url(), "tok"),
            server.url(),
            server.url(),
            Duration::from_secs(5),
            Duration::from_millis(10),
        );

        let err = uploader
            .upload_blob_for_user(b"video-bytes", "video/mp4", "did:plc:user")
            .await
            .expect_err("409 without a jobId should fail");

        assert!(
            format!("{err:#}").contains("uploadVideo failed (409)"),
            "unexpected error: {err:#}"
        );
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
