# Localnet Provisioning Smoke Test

Use this runbook after the localnet lab is up and the bridge plus handle-gateway are pointed at it.

## Required Environment

```bash
export PDS_URL=https://pds.<tailnet>.ts.net
export HANDLE_ADMIN_URL=https://handles.<tailnet>.ts.net
export HANDLE_NAME=alice
export HANDLE_DID=did:plc:alice123
export ATBRIDGE_PROVISION_URL=http://127.0.0.1:3200/provision
export ATPROTO_PROVISIONING_TOKEN=local-provisioning-token
export NOSTR_PUBKEY=npub1alice
```

## What The Script Checks

`bash scripts/localnet-smoke.sh` performs:

1. PDS health check
2. handle-admin health check
3. handle registration for `username.divine.test`
4. handle readback from the admin API
5. bridge provisioning call to the internal `/provision` endpoint

## Run It

```bash
bash scripts/localnet-smoke.sh
```

Expected result:

- all steps return `2xx`
- the handle-admin API returns `alice.divine.test`
- the bridge provisioning endpoint accepts the local handle

## Failure Triage

- If the PDS health check fails, verify the PDS slice, PLC URL, and Tailscale reachability first.
- If the handle-admin API fails, check `deploy/localnet/dns/docker-compose.yml`, the generated zone volume, and the `divine-localnet-admin` logs.
- If the provisioning call fails, confirm `divine-atbridge` is running with `deploy/localnet/bridge.env.example` values and the same provisioning token that `divine-handle-gateway` uses.

## Cleanup

- Re-run the script with a new `HANDLE_NAME` if you want a fresh smoke identity.
- Use `bash scripts/localnet-down.sh` when you are done.
