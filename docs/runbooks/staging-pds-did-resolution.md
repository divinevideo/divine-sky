# Staging PDS DID Resolution Smoke

Use this runbook to reproduce the staging `rsky-pds` DID resolution failure and then verify the fix.

## Required Environment

```bash
export PDS_URL=https://pds.staging.dvines.org
export PDS_ADMIN_PASSWORD=...
export CANARY_HANDLE=atproto-canary-$(date +%s).staging.dvines.org
export CANARY_DID=did:plc:...
```

## Smoke Command

```bash
bash scripts/staging-pds-did-smoke.sh
```

The script prints the HTTP status and response body for:

1. `GET /xrpc/_health`
2. `POST /xrpc/com.atproto.server.createAccount`
3. `GET /xrpc/com.atproto.repo.describeRepo?repo=<did>`

It exits non-zero on the first failed step.

## Expected Failure Before The Fix

Before the `rsky-pds` DID-resolution fix is deployed, expect:

- `_health` returns `200`
- `createAccount` fails with a non-2xx status
- the response body or pod logs point at PLC DID document lookup or DID parsing on the `did:plc:...` input

The repo does not yet contain a committed failing capture from this runbook. After the next staging run, paste the exact failing `createAccount` status/body here.

### Captured Failure

```text
pending capture from staging canary run
```

## Expected Success After The Fix

After the patched `rsky-pds` image is deployed to staging, expect:

- `_health` returns `200`
- `createAccount` returns `2xx` for the canary DID and handle
- `describeRepo` returns the same `did:plc:...` and `handle`

Minimal success shape:

```text
PDS health: 200
Create account: 2xx
Describe repo: 200 with did + handle matching the canary input
```

## If The Failure Changes Shape

Check these next:

1. `rsky-pds` pod logs around `com.atproto.server.createAccount`
2. the DID resolution code path in the forked `rsky-pds` repo used for staging
3. the active `rsky-pds` image tag in `../divine-iac-coreconfig`
4. the staging `DATABASE_URL` and admin password secrets
5. whether the request is reaching the correct PDS pod revision
