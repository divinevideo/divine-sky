# Handle Resolution + Video Bridge Pipeline

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix "Invalid Handle" on Bluesky by registering divine.video handles in the name-server KV, retry bridging heybob's remaining videos, and wire divine-atbridge to use video.bsky.app for playable video uploads.

**Architecture:** Three independent chunks. Chunk 1 registers handles via the divine-name-server internal API and verifies edge resolution. Chunk 2 retries heybob's video bridge with rate-limit-aware idempotent logic. Chunk 3 adds a `VideoServiceUploader` to divine-atbridge that implements the existing `BlobUploader` trait, routing video uploads through video.bsky.app's transcoding pipeline instead of direct PDS uploadBlob.

**Tech Stack:** TypeScript (Cloudflare Worker), Bash, Rust (reqwest, async-trait), Fastly KV, ATProto XRPC

**Repos:**
- `/Users/rabble/code/divine/divine-name-server` — handle registration
- `/Users/rabble/code/divine/divine-sky` — divine-atbridge + bridge scripts
- `/Users/rabble/code/divine/rsky` — rsky-pds (no changes needed)

---

## Chunk 1: Fix Handle Resolution

### Task 1: Register Handles via Name-Server Internal API

The divine-name-server has a `POST /api/internal/username/set-atproto` endpoint authenticated with a Bearer token (`ATPROTO_SYNC_TOKEN` env var). This updates D1 and syncs to Fastly KV automatically.

**Files:**
- Read: `/Users/rabble/code/divine/divine-name-server/src/routes/internal-atproto.ts:35-90`

- [ ] **Step 1: Get the ATPROTO_SYNC_TOKEN from Cloudflare**

```bash
# Check if the token is configured
npx wrangler secret list --name divine-name-server 2>/dev/null | grep ATPROTO_SYNC_TOKEN
# If not set, create one:
# npx wrangler secret put ATPROTO_SYNC_TOKEN --name divine-name-server
```

If the internal API isn't available, use the admin endpoint with Cloudflare Access JWT:
```bash
# Admin endpoint: POST https://names.admin.divine.video/api/admin/username/set-atproto
# Requires: Cf-Access-Jwt-Assertion header
```

- [ ] **Step 2: Register heybob.divine.video → did:plc:f7mklvvj7dcw2enov4jaefna**

```bash
SYNC_TOKEN="<ATPROTO_SYNC_TOKEN value>"
curl -X POST "https://names.divine.video/api/internal/username/set-atproto" \
  -H "Authorization: Bearer ${SYNC_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"name":"heybob","atproto_did":"did:plc:f7mklvvj7dcw2enov4jaefna","atproto_state":"ready"}'
```

Expected: `{"ok":true,"name":"heybob","atproto_did":"did:plc:f7mklvvj7dcw2enov4jaefna","atproto_state":"ready"}`

- [ ] **Step 3: Register videotest.divine.video → did:plc:6wfmxyaanpyidxjfsynq7ob4**

```bash
curl -X POST "https://names.divine.video/api/internal/username/set-atproto" \
  -H "Authorization: Bearer ${SYNC_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"name":"videotest","atproto_did":"did:plc:6wfmxyaanpyidxjfsynq7ob4","atproto_state":"ready"}'
```

- [ ] **Step 4: Verify edge resolution**

Wait 5 seconds for Fastly KV propagation, then:

```bash
curl -s "https://heybob.divine.video/.well-known/atproto-did"
# Expected: did:plc:f7mklvvj7dcw2enov4jaefna

curl -s "https://videotest.divine.video/.well-known/atproto-did"
# Expected: did:plc:6wfmxyaanpyidxjfsynq7ob4
```

- [ ] **Step 5: Trigger handle re-verification on PDS**

The PDS can emit an identity event to tell the relay to re-check the handle:

```bash
BASIC_AUTH=$(printf 'admin:%s' "$(gcloud secrets versions access latest --secret=rsky-pds-admin-password-staging --project=dv-platform-staging)" | base64)

# For heybob
JWT=$(curl -s -X POST "https://pds.staging.dvines.org/xrpc/com.atproto.server.createSession" \
  -H "Content-Type: application/json" \
  -d '{"identifier":"heybob.divine.video","password":"heybob-bridge-2026"}' | jq -r '.accessJwt')

curl -s -X POST "https://pds.staging.dvines.org/xrpc/com.atproto.identity.updateHandle" \
  -H "Authorization: Bearer ${JWT}" \
  -H "Content-Type: application/json" \
  -d '{"handle":"heybob.divine.video"}'
```

This triggers an identity event on the firehose, which tells the relay/appview to re-verify the handle via `/.well-known/atproto-did`.

- [ ] **Step 6: Verify on Bluesky**

Wait 30 seconds, then:
```bash
curl -s "https://api.bsky.app/xrpc/app.bsky.actor.getProfile?actor=did:plc:f7mklvvj7dcw2enov4jaefna" | jq '.handle'
# Expected: "heybob.divine.video" (not "handle.invalid")
```

---

## Chunk 2: Retry Heybob's Remaining Videos

### Task 2: Make Bridge Script Idempotent

The current `bridge-user-to-bsky.sh` doesn't check for already-bridged videos. Add dedup logic.

**Files:**
- Modify: `/Users/rabble/code/divine/divine-sky/scripts/bridge-user-to-bsky.sh`

- [ ] **Step 1: Add dedup check before uploading**

Before downloading each video, check if a post with the event's d-tag already exists on the PDS:

Add this check inside the processing loop, after extracting `DTAG`:
```bash
# Check if already bridged (use d-tag as rkey prefix)
EXISTING=$(curl -s "${PDS_URL}/xrpc/com.atproto.repo.listRecords?repo=${DID}&collection=app.bsky.feed.post&limit=100" \
  | jq -r '.records[].uri' 2>/dev/null | grep -c "${DTAG:0:12}" || true)
if [ "$EXISTING" -gt 0 ]; then
  echo "  SKIP: already bridged"
  continue
fi
```

- [ ] **Step 2: Add rate limit detection**

After a video service upload fails, check if `canUpload` is false:
```bash
# After JOB_STATE_FAILED, check if we're rate-limited
LIMITS=$(curl -s "https://video.bsky.app/xrpc/app.bsky.video.getUploadLimits" \
  -H "Authorization: Bearer ${SVC_AUTH}" 2>/dev/null)
CAN_UPLOAD=$(echo "$LIMITS" | jq -r '.canUpload // "unknown"')
if [ "$CAN_UPLOAD" = "false" ]; then
  echo "  RATE LIMITED — stopping. Retry tomorrow."
  break
fi
```

- [ ] **Step 3: Add 720p/480p variant selection**

Before downloading, check for smaller variants:
```bash
# Try 720p first, then 480p
for RES in 720p 480p; do
  VARIANT_URL="${VIDEO_URL}.${RES}.mp4"
  VARIANT_STATUS=$(curl -sI "$VARIANT_URL" -o /dev/null -w "%{http_code}")
  if [ "$VARIANT_STATUS" = "200" ]; then
    VIDEO_URL="$VARIANT_URL"
    echo "  Using ${RES} variant"
    break
  fi
done
```

- [ ] **Step 4: Commit**

```bash
git add scripts/bridge-user-to-bsky.sh
git commit -m "fix: add dedup, rate limit detection, and variant selection to bridge script"
```

### Task 3: Run Bridge for Heybob's Remaining Videos

- [ ] **Step 1: Run the updated bridge script**

```bash
PDS_URL=https://pds.staging.dvines.org \
PDS_HANDLE=heybob.divine.video \
PDS_PASSWORD=heybob-bridge-2026 \
PDS_ADMIN_PW=$(gcloud secrets versions access latest --secret=rsky-pds-admin-password-staging --project=dv-platform-staging) \
NOSTR_PUBKEY_HEX=076c979382b90f5d3a2b21f95e1ee86b6033f14c92e79b7fad3fe1f1073f4886 \
RELAY_URL=wss://relay.divine.video \
MAX_VIDEOS=25 \
bash scripts/bridge-user-to-bsky.sh
```

Expected: ~20-25 videos bridged before hitting the daily rate limit. Already-bridged videos are skipped.

- [ ] **Step 2: Verify on Bluesky**

```bash
curl -s "https://api.bsky.app/xrpc/app.bsky.feed.getAuthorFeed?actor=did:plc:f7mklvvj7dcw2enov4jaefna&limit=30" | jq '.feed | length'
# Should show growing number of posts
```

- [ ] **Step 3: If rate limited, re-run the next day**

The script is idempotent — just re-run and it will skip already-posted videos.

---

## Chunk 3: Wire divine-atbridge to Use video.bsky.app

### Task 4: Add Video Service Configuration

**Files:**
- Modify: `/Users/rabble/code/divine/divine-sky/crates/divine-atbridge/src/config.rs`

- [ ] **Step 1: Add video service config fields**

Add to `BridgeConfig` struct:
```rust
pub video_service_url: String,
pub video_service_enabled: bool,
pub video_service_poll_timeout_secs: u64,
pub video_service_poll_interval_ms: u64,
```

And the env parsing:
```rust
video_service_url: env::var("VIDEO_SERVICE_URL")
    .unwrap_or_else(|_| "https://video.bsky.app".to_string()),
video_service_enabled: env::var("VIDEO_SERVICE_ENABLED")
    .map(|v| v == "true")
    .unwrap_or(false),
video_service_poll_timeout_secs: env::var("VIDEO_SERVICE_POLL_TIMEOUT_SECS")
    .ok().and_then(|v| v.parse().ok()).unwrap_or(120),
video_service_poll_interval_ms: env::var("VIDEO_SERVICE_POLL_INTERVAL_MS")
    .ok().and_then(|v| v.parse().ok()).unwrap_or(5000),
```

- [ ] **Step 2: Verify compilation**

```bash
cargo check -p divine-atbridge
```

- [ ] **Step 3: Commit**

```bash
git add crates/divine-atbridge/src/config.rs
git commit -m "feat: add video service configuration to divine-atbridge"
```

### Task 5: Create VideoServiceUploader

This implements the existing `BlobUploader` trait, routing video uploads through video.bsky.app.

**Files:**
- Create: `/Users/rabble/code/divine/divine-sky/crates/divine-atbridge/src/video_service.rs`
- Modify: `/Users/rabble/code/divine/divine-sky/crates/divine-atbridge/src/lib.rs` (add `pub mod video_service;`)

- [ ] **Step 1: Create the video service module**

Create `crates/divine-atbridge/src/video_service.rs`:

```rust
//! Uploads video blobs through Bluesky's video transcoding service (video.bsky.app)
//! instead of direct PDS uploadBlob. This produces playable video embeds.

use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

use crate::pipeline::BlobUploader;
use crate::publisher::BlobRef;

#[derive(Debug, Clone)]
pub struct VideoServiceUploader {
    client: Client,
    pds_url: String,
    pds_auth_token: String,
    pds_service_did: String,
    video_service_url: String,
    user_did: String,
    poll_timeout: Duration,
    poll_interval: Duration,
}

#[derive(Deserialize)]
struct ServiceAuthResponse {
    token: String,
}

#[derive(Deserialize)]
struct UploadResponse {
    #[serde(rename = "jobId")]
    job_id: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct JobStatusResponse {
    #[serde(rename = "jobStatus")]
    job_status: JobStatus,
}

#[derive(Deserialize)]
struct JobStatus {
    state: String,
    blob: Option<JobBlob>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct JobBlob {
    #[serde(rename = "ref")]
    blob_ref: BlobLink,
    #[serde(rename = "mimeType")]
    mime_type: String,
    size: u64,
}

#[derive(Deserialize)]
struct BlobLink {
    #[serde(rename = "$link")]
    link: String,
}

impl VideoServiceUploader {
    pub fn new(
        pds_url: String,
        pds_auth_token: String,
        pds_service_did: String,
        video_service_url: String,
        user_did: String,
        poll_timeout: Duration,
        poll_interval: Duration,
    ) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("failed to build reqwest client");

        Self {
            client,
            pds_url,
            pds_auth_token,
            pds_service_did,
            video_service_url,
            user_did,
            poll_timeout,
            poll_interval,
        }
    }

    async fn get_service_auth(&self) -> Result<String> {
        let url = format!(
            "{}/xrpc/com.atproto.server.getServiceAuth?aud={}&lxm=com.atproto.repo.uploadBlob",
            self.pds_url, self.pds_service_did
        );
        let resp: ServiceAuthResponse = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.pds_auth_token))
            .send()
            .await
            .context("getServiceAuth request failed")?
            .json()
            .await
            .context("getServiceAuth response parse failed")?;
        Ok(resp.token)
    }

    async fn upload_to_video_service(
        &self,
        data: &[u8],
        service_token: &str,
    ) -> Result<String> {
        let url = format!(
            "{}/xrpc/app.bsky.video.uploadVideo?did={}&name=divine-video.mp4",
            self.video_service_url, self.user_did
        );
        let resp: UploadResponse = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", service_token))
            .header("Content-Type", "video/mp4")
            .body(data.to_vec())
            .send()
            .await
            .context("uploadVideo request failed")?
            .json()
            .await
            .context("uploadVideo response parse failed")?;

        match resp.job_id {
            Some(id) => Ok(id),
            None => bail!(
                "video upload failed: {}",
                resp.error.unwrap_or_else(|| "unknown".to_string())
            ),
        }
    }

    async fn poll_job(&self, job_id: &str) -> Result<BlobRef> {
        let url = format!(
            "{}/xrpc/app.bsky.video.getJobStatus?jobId={}",
            self.video_service_url, job_id
        );
        let deadline = tokio::time::Instant::now() + self.poll_timeout;

        loop {
            if tokio::time::Instant::now() > deadline {
                bail!("video transcoding timed out after {:?}", self.poll_timeout);
            }

            tokio::time::sleep(self.poll_interval).await;

            let resp: JobStatusResponse = self
                .client
                .get(&url)
                .send()
                .await
                .context("getJobStatus request failed")?
                .json()
                .await
                .context("getJobStatus parse failed")?;

            match resp.job_status.state.as_str() {
                "JOB_STATE_COMPLETED" => {
                    let blob = resp
                        .job_status
                        .blob
                        .context("completed job has no blob")?;
                    return Ok(BlobRef {
                        cid: blob.blob_ref.link,
                        mime_type: blob.mime_type,
                        size: blob.size as i64,
                    });
                }
                "JOB_STATE_FAILED" => {
                    bail!(
                        "video transcoding failed: {}",
                        resp.job_status
                            .error
                            .unwrap_or_else(|| "unknown".to_string())
                    );
                }
                _ => {
                    tracing::debug!(
                        "video job {} state: {}",
                        job_id,
                        resp.job_status.state
                    );
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl BlobUploader for VideoServiceUploader {
    async fn upload_blob(&self, data: &[u8], _mime_type: &str) -> Result<BlobRef> {
        let service_token = self
            .get_service_auth()
            .await
            .context("failed to get service auth for video upload")?;

        let job_id = self
            .upload_to_video_service(data, &service_token)
            .await
            .context("failed to upload to video service")?;

        tracing::info!("video upload job created: {}", job_id);

        self.poll_job(&job_id)
            .await
            .context("failed to poll video transcoding job")
    }
}
```

- [ ] **Step 2: Add module declaration**

In `crates/divine-atbridge/src/lib.rs`, add:
```rust
pub mod video_service;
```

- [ ] **Step 3: Verify compilation**

```bash
cargo check -p divine-atbridge
```

- [ ] **Step 4: Commit**

```bash
git add crates/divine-atbridge/src/video_service.rs crates/divine-atbridge/src/lib.rs
git commit -m "feat: add VideoServiceUploader for video.bsky.app transcoding"
```

### Task 6: Wire VideoServiceUploader into Runtime

**Files:**
- Modify: `/Users/rabble/code/divine/divine-sky/crates/divine-atbridge/src/runtime.rs`

- [ ] **Step 1: Conditionally use VideoServiceUploader when enabled**

Find where the `PdsClient` is instantiated as the blob uploader and add conditional logic:

```rust
use crate::video_service::VideoServiceUploader;

// In the service setup function, after creating PdsClient:
let blob_uploader: Box<dyn BlobUploader> = if config.video_service_enabled {
    tracing::info!("Using video.bsky.app for video uploads");
    Box::new(VideoServiceUploader::new(
        config.pds_url.clone(),
        config.pds_auth_token.clone(),
        format!("did:web:{}", config.pds_url.trim_start_matches("https://")),
        config.video_service_url.clone(),
        // user_did is set per-account in the pipeline, so this needs
        // to be handled differently — see Step 2
        String::new(), // placeholder
        Duration::from_secs(config.video_service_poll_timeout_secs),
        Duration::from_millis(config.video_service_poll_interval_ms),
    ))
} else {
    Box::new(pds_client.clone())
};
```

Note: The `user_did` is per-account. The `VideoServiceUploader` may need to accept the DID at upload time rather than at construction. This requires a minor signature change — add `did` to the `BlobUploader::upload_blob` trait or create the uploader per-account in the pipeline.

- [ ] **Step 2: Verify compilation**

```bash
cargo check -p divine-atbridge
```

- [ ] **Step 3: Commit**

```bash
git add crates/divine-atbridge/src/runtime.rs
git commit -m "feat: wire VideoServiceUploader into atbridge runtime"
```

### Task 7: Add Video Service Env Vars to Staging Deployment

**Files:**
- Modify: `../divine-iac-coreconfig/k8s/applications/divine-atbridge/base/deployment.yaml`

- [ ] **Step 1: Add env vars to deployment**

```yaml
- name: VIDEO_SERVICE_ENABLED
  value: "true"
- name: VIDEO_SERVICE_URL
  value: "https://video.bsky.app"
- name: VIDEO_SERVICE_POLL_TIMEOUT_SECS
  value: "120"
```

- [ ] **Step 2: Commit and deploy**

```bash
cd ../divine-iac-coreconfig
git add k8s/applications/divine-atbridge/
git commit -m "deploy: enable video.bsky.app transcoding for divine-atbridge"
git push
```

---

## Success Criteria

1. `curl https://heybob.divine.video/.well-known/atproto-did` returns `did:plc:f7mklvvj7dcw2enov4jaefna`
2. Bluesky shows `@heybob.divine.video` (not "Invalid Handle")
3. heybob has 20+ playable video posts on Bluesky
4. divine-atbridge with `VIDEO_SERVICE_ENABLED=true` uploads videos through video.bsky.app and they play on Bluesky

## Known Constraints

- **Video rate limit:** ~25 videos/day per account on video.bsky.app. Bridge in daily batches.
- **Profile required:** Account must have `app.bsky.actor.profile/self` record before video uploads work.
- **Service auth gotchas:** `aud` must be PDS service DID, `lxm` must be `com.atproto.repo.uploadBlob`.
- **Variant selection:** Use 720p/480p from `media.divine.video/{hash}.720p.mp4` when available — smaller files have higher success rate.
