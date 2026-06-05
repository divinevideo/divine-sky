# Video Pipeline Staging Deploy

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build and deploy rsky-pds and divine-atbridge images with all accumulated fixes, then verify the complete video pipeline: account provisioning via admin API, blob upload, and video record publication visible on the ATProto network.

**Architecture:** The rsky-pds feature branch (`fix/health-endpoint-sequencer-isolation`) must be rebased onto main to pick up both the blobstore fixes (main) and the admin auth bypass + health endpoint improvements (feature branch). A single combined image is built, deployed, and smoke-tested. The divine-atbridge Basic auth fix is already on divine-sky main. Both images are pushed to Artifact Registry, pinned in coreconfig, and synced via ArgoCD. The existing `staging-pds-did-smoke.sh` script is updated to use Basic auth matching the new admin flow.

**Tech Stack:** Rust, Docker (linux/amd64), GCP Artifact Registry, Kubernetes/ArgoCD, ATProto XRPC, Bash

**Repos:**
- `/Users/rabble/code/divine/rsky` — rsky-pds source (branch: `fix/health-endpoint-sequencer-isolation`)
- `/Users/rabble/code/divine/divine-sky` — divine-atbridge source (branch: `main`)
- `/Users/rabble/code/divine/divine-iac-coreconfig` — K8s manifests (branch: `main`)

---

## Pre-Flight Checklist

Before starting, verify:
- [ ] `gcloud auth configure-docker us-central1-docker.pkg.dev` is configured
- [ ] `kubectl` context points to the staging GKE cluster
- [ ] `PDS_ADMIN_PASSWORD` is available (from `gcloud secrets versions access latest --secret=rsky-pds-admin-password-staging`)

---

## Chunk 1: Prepare and Build rsky-pds Image

### Task 1: Rebase Feature Branch onto Main

The feature branch has admin auth bypass + health fixes. Main has blobstore/GCS fixes. Both are needed in the image.

**Files:**
- No file changes — git operations only

- [ ] **Step 1: Fetch latest and rebase**

```bash
cd /Users/rabble/code/divine/rsky
git fetch divinevideo
git checkout fix/health-endpoint-sequencer-isolation
git rebase divinevideo/main
```

Expected: Clean rebase (the changes are in different files). If conflicts arise, they will be in `create_account.rs` — resolve by keeping the feature branch version since it's a superset.

- [ ] **Step 2: Verify the combined branch compiles**

```bash
cargo check -p rsky-pds
```

Expected: 0 errors (warnings OK).

- [ ] **Step 3: Push the rebased branch**

```bash
git push divinevideo fix/health-endpoint-sequencer-isolation --force-with-lease
```

- [ ] **Step 4: Commit checkpoint**

No commit needed — rebase only.

### Task 2: Build and Push rsky-pds Docker Image

**Files:**
- Read: `rsky-pds/Dockerfile` (already correct, uses `COPY . .`)

- [ ] **Step 1: Get the short SHA for the image tag**

```bash
cd /Users/rabble/code/divine/rsky
SHORT_SHA=$(git rev-parse --short HEAD)
echo "Image tag: admin-auth-${SHORT_SHA}"
```

- [ ] **Step 2: Build the image for amd64 (GKE)**

```bash
docker build --platform linux/amd64 \
  -t us-central1-docker.pkg.dev/dv-platform-staging/containers-staging/rsky-pds:admin-auth-${SHORT_SHA} \
  -f rsky-pds/Dockerfile .
```

Expected: Build succeeds. This will take 10-20 minutes (full Rust workspace compile).

- [ ] **Step 3: Push the image**

```bash
docker push us-central1-docker.pkg.dev/dv-platform-staging/containers-staging/rsky-pds:admin-auth-${SHORT_SHA}
```

Expected: Push succeeds.

---

## Chunk 2: Build and Push divine-atbridge Image

### Task 3: Build divine-atbridge Image

The Basic auth fix (Bearer → Basic) is already on divine-sky main.

**Files:**
- Read: Dockerfile for divine-atbridge (check if it exists)

- [ ] **Step 1: Locate or create the Dockerfile**

```bash
cd /Users/rabble/code/divine/divine-sky
ls crates/divine-atbridge/Dockerfile 2>/dev/null || ls Dockerfile.atbridge 2>/dev/null
```

If no Dockerfile exists, check how the previous image (`divine-atbridge:6efafb3`) was built:

```bash
grep -r "divine-atbridge" ../divine-iac-coreconfig/k8s/ | grep -i image
```

Check if there's a Cloud Build config or a shared Dockerfile.

- [ ] **Step 2: Build the image**

```bash
SHORT_SHA=$(git rev-parse --short HEAD)
# Adjust the docker build command based on what Step 1 finds
docker build --platform linux/amd64 \
  -t us-central1-docker.pkg.dev/dv-platform-staging/containers-staging/divine-atbridge:${SHORT_SHA} \
  -f crates/divine-atbridge/Dockerfile .
```

Or if Cloud Build was used previously:

```bash
gcloud builds submit --config=cloudbuild-atbridge.yaml --substitutions=SHORT_SHA=${SHORT_SHA}
```

- [ ] **Step 3: Push the image**

```bash
docker push us-central1-docker.pkg.dev/dv-platform-staging/containers-staging/divine-atbridge:${SHORT_SHA}
```

---

## Chunk 3: Deploy to Staging

### Task 4: Pin New Images in Coreconfig

**Files:**
- Modify: `../divine-iac-coreconfig/k8s/applications/rsky-pds/overlays/staging/kustomization.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/divine-atbridge/overlays/staging/kustomization.yaml` (if exists)

- [ ] **Step 1: Update rsky-pds image tag**

In `../divine-iac-coreconfig/k8s/applications/rsky-pds/overlays/staging/kustomization.yaml`, change:

```yaml
images:
  - name: rsky-pds
    newName: us-central1-docker.pkg.dev/dv-platform-staging/containers-staging/rsky-pds
    newTag: admin-auth-<SHORT_SHA>   # was: blob-cid-fix
```

- [ ] **Step 2: Update divine-atbridge image tag**

Find and update the atbridge kustomization similarly.

- [ ] **Step 3: Commit and push coreconfig**

```bash
cd ../divine-iac-coreconfig
git add -A k8s/applications/
git commit -m "deploy(staging): rsky-pds @ admin-auth-<SHA>, divine-atbridge @ <SHA>"
git push
```

- [ ] **Step 4: Force ArgoCD sync and wait for rollout**

```bash
kubectl annotate application rsky-pds -n argocd argocd.argoproj.io/refresh=hard --overwrite
kubectl rollout status deployment/rsky-pds -n sky --timeout=120s
kubectl annotate application divine-atbridge -n argocd argocd.argoproj.io/refresh=hard --overwrite 2>/dev/null
kubectl rollout status deployment/divine-atbridge -n sky --timeout=120s 2>/dev/null
```

- [ ] **Step 5: Verify deployed config**

```bash
kubectl exec -n sky deploy/rsky-pds -- env | grep -E "INVITE|HANDLE_DOMAINS"
# Expected: PDS_INVITE_REQUIRED=true, PDS_SERVICE_HANDLE_DOMAINS=.divine.video
```

---

## Chunk 4: Fix Smoke Test and Verify Account Provisioning

### Task 5: Update Smoke Test to Use Basic Auth

The existing `staging-pds-did-smoke.sh` uses `Authorization: Bearer $PDS_ADMIN_PASSWORD` but rsky-pds expects HTTP Basic auth for admin operations.

**Files:**
- Modify: `scripts/staging-pds-did-smoke.sh`

- [ ] **Step 1: Update createAccount auth header**

Change line 103 from:

```bash
  -H "Authorization: Bearer $PDS_ADMIN_PASSWORD" \
```

to:

```bash
  -H "Authorization: Basic $(printf 'admin:%s' "$PDS_ADMIN_PASSWORD" | base64)" \
```

- [ ] **Step 2: Update describeRepo auth header**

Change line 120 from:

```bash
  -H "Authorization: Bearer $PDS_ADMIN_PASSWORD"
```

to:

```bash
  -H "Authorization: Basic $(printf 'admin:%s' "$PDS_ADMIN_PASSWORD" | base64)"
```

- [ ] **Step 3: Remove email and password from the DID-import payload**

When `CANARY_DID` is set (the admin import path), the payload should only need `did` and `handle` — the rsky-pds admin bypass now generates placeholder email and random password. Update the payload construction (lines 78-84):

```bash
if [[ -n "${CANARY_DID:-}" ]]; then
  create_account_payload="$(
    printf '{"handle":"%s","did":"%s"}' \
      "$CANARY_HANDLE" \
      "$CANARY_DID"
  )"
```

And make `CANARY_EMAIL` and `CANARY_PASSWORD` optional when `CANARY_DID` is set.

- [ ] **Step 4: Commit**

```bash
git add scripts/staging-pds-did-smoke.sh
git commit -m "fix: use Basic auth and minimal payload in PDS smoke test"
```

### Task 6: Run Smoke Test Against Staging

- [ ] **Step 1: Get admin password**

```bash
PDS_ADMIN_PASSWORD=$(gcloud secrets versions access latest --secret=rsky-pds-admin-password-staging --project=dv-platform-staging)
```

- [ ] **Step 2: Run smoke test with a pre-minted DID**

First, check if there are any existing test DIDs we can reuse:

```bash
kubectl run psql-check --rm -it --restart=Never -n sky --image=postgres:16-alpine -- \
  psql "$DATABASE_URL" -c "SELECT did, handle FROM pds.actor WHERE \"takedownRef\" IS NULL ORDER BY handle;"
```

Then run the smoke test with an existing test DID (e.g., vinetest.divine.video):

```bash
PDS_URL=https://pds.staging.dvines.org \
PDS_ADMIN_PASSWORD="$PDS_ADMIN_PASSWORD" \
CANARY_HANDLE=smoketest.divine.video \
CANARY_DID=did:plc:$(openssl rand -hex 16 | head -c 24) \
bash scripts/staging-pds-did-smoke.sh
```

Expected: `createAccount` succeeds (returns 200 with DID and handle), `describeRepo` succeeds.

- [ ] **Step 3: If createAccount fails, check PDS logs**

```bash
kubectl logs -n sky deploy/rsky-pds --tail=50
```

Common failure modes:
- `InvalidInviteCode` → admin bypass not deployed (check image tag)
- `UnsupportedDomain` → handle domain not in `PDS_SERVICE_HANDLE_DOMAINS` (must be `.divine.video`)
- DID resolution timeout → `PDS_ID_RESOLVER_TIMEOUT` may need increase, or PLC directory unreachable
- `BadAuth` → Basic auth format wrong

---

## Chunk 5: End-to-End Video Publishing Test

### Task 7: Manual Video Blob Upload and Record Creation

Test the full blob → record pipeline manually before testing via divine-atbridge.

**Files:**
- Modify: `scripts/post-vine-to-pds.sh` (update auth to Basic)

- [ ] **Step 1: Upload a test video blob**

```bash
# Download a small test video (or use an existing one)
curl -o /tmp/test-video.mp4 "https://media.divine.video/<known-sha256>" 2>/dev/null \
  || curl -o /tmp/test-video.mp4 "https://test-videos.co.uk/vids/bigbuckbunny/mp4/h264/360/Big_Buck_Bunny_360_10s_1MB.mp4"

# Upload blob to PDS
BASIC_AUTH=$(printf 'admin:%s' "$PDS_ADMIN_PASSWORD" | base64)
TEST_DID="<did from Task 6>"  # use the DID created in the smoke test

curl -v -X POST "https://pds.staging.dvines.org/xrpc/com.atproto.repo.uploadBlob" \
  -H "Authorization: Basic ${BASIC_AUTH}" \
  -H "Content-Type: video/mp4" \
  --data-binary @/tmp/test-video.mp4
```

Expected: 200 response with blob ref containing CID (should start with `bafkrei`).

- [ ] **Step 2: Verify blob is retrievable**

```bash
BLOB_CID="<cid from step 1>"
curl -v "https://pds.staging.dvines.org/xrpc/com.atproto.sync.getBlob?did=${TEST_DID}&cid=${BLOB_CID}" \
  -o /tmp/retrieved-video.mp4

# Verify sizes match
ls -la /tmp/test-video.mp4 /tmp/retrieved-video.mp4
```

Expected: Retrieved file matches uploaded file size.

- [ ] **Step 3: Create a video post record**

```bash
RKEY="test-$(date +%s)"
curl -v -X POST "https://pds.staging.dvines.org/xrpc/com.atproto.repo.createRecord" \
  -H "Authorization: Basic ${BASIC_AUTH}" \
  -H "Content-Type: application/json" \
  -d "{
    \"repo\": \"${TEST_DID}\",
    \"collection\": \"app.bsky.feed.post\",
    \"rkey\": \"${RKEY}\",
    \"record\": {
      \"\$type\": \"app.bsky.feed.post\",
      \"text\": \"Test video from divine.video staging\",
      \"createdAt\": \"$(date -u +%Y-%m-%dT%H:%M:%S.000Z)\",
      \"embed\": {
        \"\$type\": \"app.bsky.embed.video\",
        \"video\": {
          \"\$type\": \"blob\",
          \"ref\": { \"\$link\": \"${BLOB_CID}\" },
          \"mimeType\": \"video/mp4\",
          \"size\": $(stat -f%z /tmp/test-video.mp4 2>/dev/null || stat -c%s /tmp/test-video.mp4)
        },
        \"aspectRatio\": { \"width\": 16, \"height\": 9 }
      }
    }
  }"
```

Expected: 200 response with `uri` and `cid` of the created record.

- [ ] **Step 4: Verify record exists**

```bash
curl -s "https://pds.staging.dvines.org/xrpc/com.atproto.repo.getRecord?repo=${TEST_DID}&collection=app.bsky.feed.post&rkey=${RKEY}" | jq .
```

Expected: Record with video embed is returned.

- [ ] **Step 5: Check if the Bluesky relay received the event**

```bash
# If PDS_CRAWLERS is set to https://bsky.network, the relay should have it
curl -s "https://api.bsky.app/xrpc/app.bsky.feed.getPostThread?uri=at://${TEST_DID}/app.bsky.feed.post/${RKEY}" | jq .error
```

Expected: Either the post is visible, or an error like `NotFound` if the relay hasn't indexed a DID from our PDS yet (this is expected for new DIDs not yet in the network). The important thing is that the PDS accepted and stored the record correctly.

### Task 8: Document Results and Known Gaps

- [ ] **Step 1: Update staging status doc**

**Files:**
- Modify: `docs/runbooks/divine-sky-staging-status.md`

Update with:
- New image tags deployed
- Results of smoke test (createAccount pass/fail)
- Results of video upload test (uploadBlob, getBlob, createRecord)
- Whether relay picked up the events
- Any remaining blockers for the full divine-atbridge pipeline

- [ ] **Step 2: Commit**

```bash
git add docs/runbooks/divine-sky-staging-status.md scripts/
git commit -m "docs: update staging status after video pipeline testing"
```

---

## Success Criteria

1. `createAccount` with admin Basic auth + pre-minted DID succeeds on staging rsky-pds
2. `uploadBlob` stores a video blob and returns a `bafkrei...` CID
3. `getBlob` retrieves the blob with matching content
4. `createRecord` with `app.bsky.embed.video` succeeds
5. `getRecord` returns the video post with correct embed structure

## Known Follow-ups (Out of Scope)

- **Full divine-atbridge automated flow**: Requires the rest of the remediation plan (Chunks 2-6) — handle gateway, name server, keycast wiring
- **Bluesky relay visibility**: New DIDs from our PDS may need manual crawl registration or time to be discovered
- **Video transcoding**: Bluesky's video service transcodes videos for playback; verifying transcoded output is a separate concern
- **Production deployment**: This plan covers staging only
