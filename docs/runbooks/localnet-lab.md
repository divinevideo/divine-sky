# Localnet Lab

Use this runbook when you need the full ATProto localnet lab instead of the fast bridge-oriented stack in `config/docker-compose.yml`.

## Required Local Dependencies

- Docker with Compose v2
- Rust toolchain
- PostgreSQL client libraries for local Rust test runs
- Tailscale installed locally, plus permission to authenticate the lab sidecars

## Expected Hostnames

- `plc.<tailnet>.ts.net`
- `pds.<tailnet>.ts.net`
- `jetstream.<tailnet>.ts.net`
- `handles.<tailnet>.ts.net`
- `username.divine.test`

## Bring-Up Order

Run:

```bash
bash scripts/localnet-up.sh
```

The script starts PLC, then PDS, then Jetstream, then DNS. It does not complete Tailscale auth or certificate issuance for you.

## Tailscale Prerequisites

- Set real `TS_AUTHKEY` values instead of the checked-in placeholders.
- Approve each tailnet node if your auth policy requires manual confirmation.
- Provision TLS certificates for the nginx proxies or replace the cert paths in the slice configs.

## Create A Local Handle Mapping

Once the DNS admin endpoint is reachable, register a handle:

```bash
curl -X POST "https://handles.<tailnet>.ts.net/api/handles" \
  -H "Content-Type: application/json" \
  -d '{"name":"alice","did":"did:plc:alice123"}'
```

That creates `alice.divine.test` plus `_atproto.alice.divine.test` TXT data in the generated zone file that CoreDNS reads.

## Point `divine-atbridge` At The Lab

Load `deploy/localnet/bridge.env.example`, then replace the placeholder hostnames with your tailnet hostnames before starting `divine-atbridge`.

Important differences from the fast stack:

- `PLC_DIRECTORY_URL` targets the local PLC slice
- `PDS_URL` targets the localnet PDS
- `HANDLE_DOMAIN=divine.test`
- `RELAY_SOURCE_NAME=localnet-relay`

## Point `divine-handle-gateway` At The Lab

Load `deploy/localnet/handle-gateway.env.example`, then replace any local sibling-repo URLs as needed for your workstation layout.

The user-facing flow still depends on sibling repos:

- `../keycast`
- `../divine-name-server`
- `../divine-router`

`divine-handle-gateway` stays an internal control-plane service. It does not serve the public `/.well-known/atproto-did` route.

## Cleanup And Reset

Stop the lab:

```bash
bash scripts/localnet-down.sh
```

If you need a deeper reset:

1. Run the stop script.
2. Remove the named Docker volumes for the slice you want to reset.
3. Recreate any handle mappings through the handle-admin API before rerunning the smoke flow.
