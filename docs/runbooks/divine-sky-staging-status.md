# Divine Sky Staging Status — 2026-03-21

## ATProto Remediation Tracker

| Area | Current State | Evidence | Next Action |
|------|---------------|----------|-------------|
| Staging PDS canary smoke | Reproduced against live staging and captured in the runbook | `scripts/staging-pds-did-smoke.sh`, `docs/runbooks/staging-pds-did-resolution.md` | deploy the patched `rsky-pds` image, then rerun the same smoke flow until `describeRepo` passes |
| `divine-atbridge` and `divine-handle-gateway` tests | Verified locally and immutable images built | `control_plane`, `provision_api`, and `provisioning_lifecycle` all passed; staging images `divine-handle-gateway:6efafb3` and `divine-atbridge:6efafb3` were pushed to Artifact Registry | replace `latest` pins in staging and sync the overlays |
| `divine-name-server` internal sync + cron sync | Code-ready locally, deploy pending | local Vitest slice passes for Task 4 and Task 5 | apply D1 migration and deploy worker |
| `divine-router` DID edge handler | Code-ready locally, README still being tightened, publish pending | route and tests exist on `feat/atproto-handle-resolution` | publish Fastly service and verify canary DID resolution |
| keycast ATProto image | Image already pinned to immutable tag | `0e5b6cb34dad075011d3703836ca111ceb583aa8` in staging overlay | keep tag, verify staging endpoints after runtime secret wiring |
| keycast runtime secret wiring | Landed locally in `divine-iac-coreconfig`, not yet synced | new `keycast-atproto-runtime` secret contract and env vars | sync staging overlay and verify `/api/user/atproto/*` against real control plane |

## Running Services

| Service | Namespace | Pods | Public URL | Status |
|---------|-----------|------|------------|--------|
| divine-labeler | divine-labeler | 2/2 | labels.staging.dvines.org | Healthy, serving labels |
| divine-feedgen | sky | 2/2 | feed.staging.dvines.org | Healthy |
| divine-handle-gateway | sky | 2/2 | internal | Healthy |
| divine-atbridge | sky | 1/1 | internal | Running, connected to relay |
| rsky-pds | sky | 1/1 | pds.staging.dvines.org | Restarted on 2026-03-21 and is now intermittently failing `_health`; earlier `createAccount` traces still show the PLC DID-resolution failure |

## Labeler DID

- `did:plc:diipfbfrgwpbpoeehyovemmy`
- Registered on plc.directory
- Handle: labeler.staging.dvines.org

## What's Working End-to-End

- Moderation webhook → labeler → signed labels → queryLabels
- Tested: `POST /webhook/moderation-result` → 200, labels appear in `GET /xrpc/com.atproto.label.queryLabels`
- Cloudflare Worker secrets configured (ATPROTO_LABELER_WEBHOOK_URL, ATPROTO_LABELER_TOKEN)

## What Needs Debugging: rsky-pds

**Problem:** `createAccount` reaches PLC minting, then fails while resolving the freshly minted DID document.

**Confirmed findings:**
1. The real failure path is the PLC-minting path with `did` omitted from `createAccount`, not the imported-account path with a pre-existing DID.
2. The `rabble/rsky` fork already contains the DID-path and S3 fixes, but `rsky-pds/Dockerfile` builds from upstream `blacksky-algorithms/rsky` and only copies `rsky-pds/src`, so the patched `rsky-identity` code never reaches the staging image.
3. The live deployment is manually drifted beyond `../divine-iac-coreconfig`; secret-backed signing keys and AWS credentials are not declared in Git yet.
4. Staging sets `PDS_ID_RESOLVER_TIMEOUT=30000`, but upstream `rsky-pds/src/lib.rs` currently constructs `IdResolver::new(IdentityResolverOpts { timeout: None, ... })`, so the resolver still falls back to the library default timeout unless the fork is patched.

**What worked before the latest pod restart:** `_health` returned `200`, PLC operations were accepted by `plc.directory`, and the failure was narrowed to the image/runtime path after PLC minting.

**Current staging drift as of 2026-03-21:** the replacement `rsky-pds` pod restarted and then began failing readiness/liveness probes on `/xrpc/_health`, so the public endpoint can now return `503` before the createAccount path is even exercised.

**To fix:**
1. Build and push a new `rsky-pds` image from the fork after fixing its Dockerfile to compile the local workspace.
2. Pin that image in `../divine-iac-coreconfig` instead of `latest`.
3. Reconcile the missing `rsky-pds` runtime secrets and env vars into `../divine-iac-coreconfig` so ArgoCD does not roll staging back to an incomplete deployment.
4. Re-run `bash scripts/staging-pds-did-smoke.sh` until `createAccount` and `describeRepo` both pass.

**Database state:**
- `rsky_pds` database exists with PDS schema (`pds.*` tables from rsky migrations)
- GCP secret `rsky-pds-database-url-staging` v2 points to rsky_pds DB

## What's Next After PDS Fix

1. Verify `com.atproto.server.describeServer` returns valid JSON
2. Create a test account via PDS admin API
3. Trigger user opt-in via handle-gateway
4. Verify atbridge provisions DID + PDS account
5. Verify content mirroring: Nostr video event → ATProto record
6. Verify moderation flow: classification → labeler webhook → label on mirrored content

## GCP Secrets Created (staging)

| Secret | Purpose |
|--------|---------|
| divine-labeler-signing-key-staging | Labeler ECDSA signing key |
| divine-labeler-webhook-token-staging | Webhook auth token |
| divine-labeler-db-password-staging | Labeler DB password |
| rsky-pds-database-url-staging | PDS PostgreSQL URL |
| rsky-pds-jwt-secret-staging | PDS JWT signing secret |
| rsky-pds-admin-password-staging | PDS admin password |
| divine-atbridge-*-staging | 10 secrets for atbridge |
| divine-handle-gateway-*-staging | 7 secrets for handle-gateway |

## GCS Buckets

- `divine-pds-blobs-staging` — PDS blob storage (empty, created for rsky-pds)
- `media.divine.video` bucket — Existing blossom CDN (not used directly by PDS)

## Container Images (Artifact Registry: dv-platform-staging)

| Image | Built Via |
|-------|-----------|
| divine-labeler:latest | Local Docker |
| divine-feedgen:latest | Local Docker |
| divine-handle-gateway:6efafb3 | Cloud Build |
| divine-atbridge:6efafb3 | Cloud Build |
| rsky-pds:latest | Cloud Build (E2_HIGHCPU_32) |
| keycast:`0e5b6cb34dad075011d3703836ca111ceb583aa8` | Immutable staging pin already present |

## Pending Runtime Verifications

- `rsky-pds` patched image tag and rollout timestamp: pending
- `divine-name-server` worker deploy timestamp: pending
- `divine-router` publish timestamp: pending
- keycast `/api/user/atproto/enable|status|disable` staging verification: pending
- final canary username and DID for end-to-end rollout record: pending
