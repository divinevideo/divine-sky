# Staging PDS DID Resolution Smoke

Use this runbook to reproduce the staging `rsky-pds` DID resolution failure and then verify the fix.

If the current staging pod is already failing `/xrpc/_health`, restore basic pod health first and then rerun this flow. The captured failure below is from the last staging replay where `_health` still returned `200` and the request progressed into PLC DID minting.

## Required Environment

```bash
export PDS_URL=https://pds.staging.dvines.org
export PDS_ADMIN_PASSWORD=...
export CANARY_HANDLE=atproto-canary-$(date +%s).staging.dvines.org
export CANARY_EMAIL=atproto-canary-$(date +%s)@example.com
export CANARY_PASSWORD='replace-with-a-random-canary-password'
```

Leave `CANARY_DID` unset to reproduce the staging PLC-minting path. Only set `CANARY_DID` when testing the imported-account path with a pre-existing DID.

## Smoke Command

```bash
bash scripts/staging-pds-did-smoke.sh
```

The script prints the HTTP status and response body for:

1. `GET /xrpc/_health`
2. `POST /xrpc/com.atproto.server.createAccount`
3. `GET /xrpc/com.atproto.repo.describeRepo?repo=<did-from-createAccount>`

It exits non-zero on the first failed step.

## Expected Failure Before The Fix

Before the `rsky-pds` DID-resolution fix is deployed, expect:

- `_health` returns `200`
- `createAccount` fails with a non-2xx status when `did` is omitted and the PDS tries to mint a PLC DID
- the public response comes back as `503 Service Temporarily Unavailable`
- the pod logs point at PLC DID document lookup immediately after `Succesfully sent PLC Operation`

### Captured Failure

```text
Run date: 2026-03-21
Handle: canary-012499.staging.dvines.org
Email: canary-012499@example.com

== PDS health ==
Status: 200
Body:
{"version":"0.3.0-beta.3"}

== Create account ==
Status: 503
Body:
<html>
<head><title>503 Service Temporarily Unavailable</title></head>
<body>
<center><h1>503 Service Temporarily Unavailable</h1></center>
<hr><center>nginx</center>
</body>
</html>

Step failed: Create account
```

Matching `rsky-pds` pod log excerpt from the failing createAccount flow:

```text
2026-03-20T22:18:39.045273Z INFO  Creating new user account
2026-03-20T22:18:47.443462Z INFO  Succesfully sent PLC Operation
2026-03-20T22:18:53.192811Z ERROR @LOG: failed to resolve did doc for `did:plc:j2ah7kkvgko6uqm255in5dbq` with error: `error sending request for url (https://plc.directory/did:plc:j2ah7kkvgko6uqm255in5dbq)`
```

## Expected Success After The Fix

After the patched `rsky-pds` image is deployed to staging, expect:

- `_health` returns `200`
- `createAccount` returns `2xx` for the canary handle, email, and password and returns a minted `did:plc:...`
- `describeRepo` returns the same minted `did:plc:...` and `handle`

Minimal success shape:

```text
PDS health: 200
Create account: 2xx with did + handle in the response body
Describe repo: 200 with did + handle matching the createAccount response
```

## If The Failure Changes Shape

Check these next:

1. `rsky-pds` pod logs around `com.atproto.server.createAccount`
2. the `rsky-pds` identity-resolver wiring in `src/lib.rs`; staging sets `PDS_ID_RESOLVER_TIMEOUT=30000`, but the resolver still uses the library default timeout unless the fork is patched
3. the handle-domain validation in `src/apis/com/atproto/server/mod.rs`; staging needs `PDS_SERVICE_HANDLE_DOMAINS=.staging.dvines.org,.divine.video` even though the PDS endpoint is `pds.staging.dvines.org`
4. the active `rsky-pds` image tag in `../divine-iac-coreconfig`
5. whether the request is reaching the correct PDS pod revision
