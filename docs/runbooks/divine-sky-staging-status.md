# Divine Sky Staging Status — 2026-03-21

## ATProto Remediation Tracker

| Area | Current State | Evidence | Next Action |
|------|---------------|----------|-------------|
| Staging PDS canary smoke | Scripted locally, not yet run from this repo state | `scripts/staging-pds-did-smoke.sh`, `docs/runbooks/staging-pds-did-resolution.md` | Run the canary with staging admin credentials and paste the failing or passing capture into the runbook |
| `divine-atbridge` and `divine-handle-gateway` tests | Verified locally with `libpq` bootstrap | `control_plane`, `provision_api`, and `provisioning_lifecycle` all passed with Homebrew `libpq` env configured | build immutable staging images from the tested revisions and replace `latest` pins |
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
| rsky-pds | sky | 1/1 (old pod) | pds.staging.dvines.org | Needs debugging (see below) |

## Labeler DID

- `did:plc:diipfbfrgwpbpoeehyovemmy`
- Registered on plc.directory
- Handle: labeler.staging.dvines.org

## What's Working End-to-End

- Moderation webhook → labeler → signed labels → queryLabels
- Tested: `POST /webhook/moderation-result` → 200, labels appear in `GET /xrpc/com.atproto.label.queryLabels`
- Cloudflare Worker secrets configured (ATPROTO_LABELER_WEBHOOK_URL, ATPROTO_LABELER_TOKEN)

## What Needs Debugging: rsky-pds

**Problem:** New rsky-pds pods crash with exit code 137 (OOM) or liveness probe failures.

**Root cause candidates:**
1. Resource limits may be too low (currently 512Mi memory limit). rsky-pds with Rocket + Diesel + AWS SDK might need more.
2. The `sh -c "ROCKET_PORT=8000 ROCKET_ADDRESS=0.0.0.0 /usr/local/bin/rsky-pds"` command override may interact badly with Rocket's config parsing.
3. Database connection to `rsky_pds` DB may be failing (the old pod uses `divine_bridge` DB which doesn't have PDS schema).

**What works:** The OLD pod (8h uptime, using divine_bridge DB) stays running on 127.0.0.1:8000 but panics on API calls because divine_bridge doesn't have PDS tables.

**To fix:**
1. Run `bash scripts/staging-pds-did-smoke.sh` with a fresh canary DID and capture the exact failure shape.
2. Isolate the DID resolution path in the forked `rsky-pds` source from the smoke output and pod logs.
3. Pin a patched non-`latest` staging image in `../divine-iac-coreconfig`.
4. Re-run the canary smoke until `createAccount` and `describeRepo` both pass.

**Database state:**
- `rsky_pds` database exists with PDS schema (`pds.*` tables from rsky migrations)
- DB user: `rsky_pds`, password: `pds-staging-db-pw-2026`
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
| divine-handle-gateway:latest | Local Docker |
| divine-atbridge:latest | Local Docker |
| rsky-pds:latest | Cloud Build (E2_HIGHCPU_32) |
| keycast:`0e5b6cb34dad075011d3703836ca111ceb583aa8` | Immutable staging pin already present |

## Pending Runtime Verifications

- `rsky-pds` canary handle and DID used for the next smoke run: pending
- `divine-name-server` worker deploy timestamp: pending
- `divine-router` publish timestamp: pending
- keycast `/api/user/atproto/enable|status|disable` staging verification: pending
- final canary username and DID for end-to-end rollout record: pending
