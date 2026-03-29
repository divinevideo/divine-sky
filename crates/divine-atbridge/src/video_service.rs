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
    /// Admin bearer token for the PDS.
    pds_auth_token: String,
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
        pds_auth_token: String,
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
            pds_auth_token,
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

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.pds_auth_token))
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

        let url = format!(
            "{}/xrpc/com.atproto.server.describeServer",
            self.pds_url
        );
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

        let upload: UploadVideoResponse = serde_json::from_str(&body_text)
            .with_context(|| {
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
                upload
                    .error
                    .or(upload.message)
                    .unwrap_or_else(|| body_text)
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

            let body: JobStatusResponse = resp
                .json()
                .await
                .context("getJobStatus parse failed")?;

            match body.job_status.state.as_str() {
                "JOB_STATE_COMPLETED" => {
                    let blob = body
                        .job_status
                        .blob
                        .context("completed job has no blob ref")?;
                    return Ok(BlobRef::new(
                        blob.blob_ref.link,
                        blob.mime_type,
                        blob.size,
                    ));
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
                    tracing::debug!(
                        "video job {} state: {}",
                        job_id,
                        other
                    );
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
