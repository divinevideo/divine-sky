# rsky-pds Issues Discovered During Deployment

## Status: Issues 1-2 fixed in PR, Issue 6 now blocking

**Upstream PR:** https://github.com/blacksky-algorithms/rsky/pull/165
**Fork branch:** `rabble/rsky:fix/s3-bucket-and-did-resolution`

## Issue 1: URL-encoded colons in DID resolution (FIXED in PR)

**File:** `rsky-identity/src/did/plc_resolver.rs` + `rsky-pds/src/plc/mod.rs`
**Impact:** PLC client and DID resolver encode DIDs with `encode_uri_component()`, turning `did:plc:abc` into `did%3Aplc%3Aabc`

**Fix:** Remove `encode_uri_component()` — DIDs are valid URL path segments.

## Issue 2: S3 bucket naming uses DID as bucket name (FIXED in PR)

**File:** `rsky-pds/src/actor_store/aws/s3.rs`
**Impact:** Uses DID as S3 bucket name — colons are invalid in bucket names on S3/GCS.

**Fix:** Read bucket from `PDS_BLOBSTORE_S3_BUCKET` env var, keep DID as path prefix only.

## Issue 3: Missing env vars not documented

**Env vars discovered as required (not in rsky README):**
- `PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX` — secp256k1 private key for signing repos
- `PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX` — secp256k1 private key for PLC rotations
- `AWS_ENDPOINT` — S3-compatible endpoint URL
- `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` — S3 credentials (HMAC for GCS)
- `AWS_REGION` — Required by AWS SDK (use "auto" for GCS)
- `PDS_BLOBSTORE_S3_BUCKET` — S3 bucket name (our fix, not in upstream)

## Issue 4: Rocket config parsing

`ROCKET_ADDRESS` env var must be set via shell command override, not K8s env var.
Rocket's TOML parser rejects `0.0.0.0` as an invalid format when read from env var.

**Fix:** Use `command: ["sh", "-c", "ROCKET_PORT=8000 ROCKET_ADDRESS=0.0.0.0 /usr/local/bin/rsky-pds"]`

## Issue 5: PDS_HOSTNAME vs handle domain conflict

rsky-pds `validate_handle()` checks `handle.ends_with(PDS_HOSTNAME)`.
`ensure_handle_service_constraints()` requires single-label usernames (no dots before domain).

**Fix:** Set `PDS_HOSTNAME=staging.dvines.org` (handle domain, not PDS URL).

## Issue 6: reqwest HTTP client fails to resolve DIDs from inside container (CURRENT BLOCKER)

**Symptom:** After PLC operation succeeds, the identity resolver's reqwest GET to
`https://plc.directory/did:plc:xxx` fails with "error sending request".

**What works:**
- PLC POST (sending operations) succeeds from the same container
- `curl` from pods in the same namespace succeeds
- The DID exists on plc.directory (verified externally)

**What fails:**
- rsky-identity's `DidPlcResolver::resolve_no_check()` — reqwest GET request fails
- The error is not 404 — it's a connection/TLS failure

**Hypotheses:**
1. reqwest timeout too short (default `PDS_ID_RESOLVER_TIMEOUT` = 3 seconds) combined with
   AWS IMDS probe timeouts consuming async runtime capacity
2. reqwest's TLS backend (likely `rustls` or `native-tls`) has different behavior than curl
3. The reqwest client is created fresh per request (`reqwest::Client::new()`) which is expensive
   and may hit connection pool limits

**Debug steps for next session:**
```bash
# Add RUST_BACKTRACE for full error chain
kubectl patch deployment rsky-pds -n sky --type json -p '[
  {"op":"add","path":"/spec/template/spec/containers/0/env/-","value":{"name":"RUST_BACKTRACE","value":"full"}}
]'

# Increase resolver timeout
kubectl patch deployment rsky-pds -n sky --type json -p '[
  {"op":"add","path":"/spec/template/spec/containers/0/env/-","value":{"name":"PDS_ID_RESOLVER_TIMEOUT","value":"30000"}}
]'

# Try disabling AWS IMDS probes (they eat startup time)
kubectl patch deployment rsky-pds -n sky --type json -p '[
  {"op":"add","path":"/spec/template/spec/containers/0/env/-","value":{"name":"AWS_EC2_METADATA_DISABLED","value":"true"}}
]'
```

## DIDs Created on plc.directory

| DID | Handle | Status |
|-----|--------|--------|
| `did:plc:diipfbfrgwpbpoeehyovemmy` | labeler.staging.dvines.org | Active (labeler service) |
| `did:plc:xk57u6jwoquak2glzuzhgmew` | testaccount.staging.dvines.org | Orphaned (not in PDS DB) |
| `did:plc:7ab7fexo5rjwswq36yq2tur3` | testaccount2.staging.dvines.org | Orphaned |
| `did:plc:uqq5clhtqwcvd3aicfdredq5` | bridgetest.staging.dvines.org | Orphaned |
| `did:plc:osiyqfbge7scmom2akauk6wj` | divinetestbridge.staging.dvines.org | Orphaned |
| `did:plc:yiag7my7lgppn7tizcy4jym6` | divinebridge.staging.dvines.org | Orphaned |

## Staging Deployment State

All services running:
- rsky-pds: 1/1 at `pds.staging.dvines.org` (health OK, account creation blocked by issue 6)
- divine-labeler: 2/2 at `labels.staging.dvines.org` (fully operational)
- divine-feedgen: 2/2 at `feed.staging.dvines.org` (fully operational)
- divine-handle-gateway: 2/2 internal (ready)
- divine-atbridge: 1/1 internal (connected to relay, needs working PDS)

## GCS Resources

- Bucket: `divine-pds-blobs-staging` (for PDS blob storage)
- HMAC Key ID: `GOOG1E3B4NUHTI4SMX7AZNAG5WX6WKZTPEFVS6SK6M66HZRESSHDUIZP7SNQ4`
- Service Account: `rsky-pds-staging@dv-platform-staging.iam.gserviceaccount.com`
