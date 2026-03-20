# Divine Sky Staging Status — 2026-03-21

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
1. Increase memory limits to 1Gi in the deployment
2. Verify the `rsky_pds` DB has all tables: `kubectl run ... psql -c "\dt pds.*"`
3. Check if the ExternalSecret is fetching the correct DATABASE_URL version (v2 points to rsky_pds)
4. Consider removing the `command` override and using a Rocket.toml config file baked into the Docker image instead

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
