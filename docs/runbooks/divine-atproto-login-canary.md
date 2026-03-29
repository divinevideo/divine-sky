# Divine ATProto Login Canary

## Purpose

Use this runbook to validate the handle-first login contract for a Divine ATProto account before treating a deployment as production-ready.

The required split is:

- `username.divine.video` for public handle resolution
- `login.divine.video` for the human console
- `pds.divine.video` for the PDS
- `entryway.divine.video` for ATProto authorization

## Canary Account

Use `rabble.divine.video` as the first canary until the multi-user migration path is proven.

## Checks

Run these commands in order:

```bash
bash scripts/smoke-divine-atproto-login.sh
```

Expected:

- handle resolution and `/.well-known/atproto-did` agree
- the DID document points at `https://pds.divine.video`
- `pds.divine.video` returns JSON from `describeServer`
- `pds.divine.video` returns JSON protected-resource metadata
- `entryway.divine.video` returns JSON authorization-server metadata

Then verify the canary account still resolves publicly:

```bash
curl -fsS https://rabble.divine.video/.well-known/atproto-did
curl -fsS "https://public.api.bsky.app/xrpc/com.atproto.identity.resolveHandle?handle=rabble.divine.video"
```

Expected:

- both commands return the same DID

## Failure Modes

- If `login.divine.video` returns HTML for any ATProto discovery path, the login contract is broken.
- If the DID document still points to a staging PDS host, the account is not ready for handle-first login.
- If protected-resource metadata is missing, OAuth discovery cannot continue.
- If authorization-server metadata is missing, the client cannot complete the login flow.
