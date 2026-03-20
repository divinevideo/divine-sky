# rsky-pds Issues Discovered During Deployment

## Issue 1: URL-encoded colons in DID resolution (BLOCKING)

**File:** rsky-pds/src/apis/com/atproto/server/mod.rs → `safe_resolve_did_doc()`
**Impact:** Account creation fails at DID doc resolution step after PLC operation succeeds

The rsky identity resolver URL-encodes the DID when making HTTP requests to plc.directory.
Instead of `https://plc.directory/did:plc:abc123`, it sends `https://plc.directory/did%3Aplc%3Aabc123`,
which returns an error.

**Workaround:** None in configuration. Requires rsky code fix.

## Issue 2: S3 bucket naming uses DID as bucket name

**File:** rsky-pds/src/actor_store/aws/s3.rs
**Impact:** Cannot use standard S3/GCS because DIDs contain colons (invalid in bucket names)

rsky-pds sets `bucket: did` for each actor's S3BlobStore. This means it tries to create/access
S3 buckets named like `did:plc:abc123` — which is an invalid bucket name on AWS S3, GCS, and
most S3-compatible providers.

This was designed for DigitalOcean Spaces where the "bucket" concept works differently
(Spaces may handle colons differently, or the implementation may use a single Space
with DID-based prefixes).

**Workaround options:**
1. Fork rsky-pds and change to use a single bucket with DID-prefixed paths
2. Use MinIO (S3-compatible object storage) deployed in-cluster, configured to accept colons
3. Use DigitalOcean Spaces as the blob backend
4. Use a reverse proxy that translates bucket names to path prefixes

## Issue 3: Missing env vars not documented

**Env vars discovered as required (not in README):**
- `PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX` — secp256k1 private key for signing repos
- `PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX` — secp256k1 private key for PLC rotations
- `AWS_ENDPOINT` — S3-compatible endpoint URL
- `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` — S3 credentials (HMAC for GCS)
- `AWS_REGION` — Required by AWS SDK (use "auto" for GCS)

## Issue 4: Rocket config parsing

`ROCKET_ADDRESS` env var must be set via shell command override, not K8s env var.
Rocket's TOML parser rejects `0.0.0.0` as an invalid format when read from env var.

**Fix:** Use `command: ["sh", "-c", "ROCKET_PORT=8000 ROCKET_ADDRESS=0.0.0.0 /usr/local/bin/rsky-pds"]`

## Issue 5: PDS_HOSTNAME vs handle domain conflict

rsky-pds's `validate_handle()` checks `handle.ends_with(PDS_HOSTNAME)`, but the service
handle domain check requires single-label usernames (no dots in username part).

Setting PDS_HOSTNAME to the handle domain (e.g., `staging.dvines.org`) resolves this,
but the PDS's own public URL then uses this hostname for other purposes.

**Fix:** Set `PDS_HOSTNAME=staging.dvines.org` (handle domain) and use `PDS_SERVICE_DID` for the PDS's DID identity.

## Current State

DIDs created on plc.directory (account creation partially succeeded):
- `did:plc:xk57u6jwoquak2glzuzhgmew` → testaccount.staging.dvines.org
- `did:plc:7ab7fexo5rjwswq36yq2tur3` → testaccount2.staging.dvines.org
- `did:plc:uqq5clhtqwcvd3aicfdredq5` → bridgetest.staging.dvines.org

These DIDs exist on plc.directory but the accounts don't exist in the PDS database.
They'll need to be recreated once the rsky-pds issues are fixed (the DIDs will get new ones
since the old ones point to the wrong PDS endpoint).

## Recommended Next Steps

1. **Fork rsky-pds** and fix issues 1 and 2 (URL encoding + S3 bucket naming)
2. Or **deploy MinIO** in-cluster as an S3-compatible blob store that handles DID-named buckets
3. Or **switch to the official TypeScript PDS** (`@atproto/pds`) which is more mature and well-documented
