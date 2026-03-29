# login.divine.video

## Purpose

`login.divine.video` is the human-facing DiVine account console. It owns username claim state, consent, ATProto lifecycle state, and the user-facing enable/status/disable API, but it is not the public ATProto protocol origin.

It does not serve `/.well-known/atproto-did`. That read-only host resolution now belongs to `divine-router`, which reads the public state published by `divine-name-server`.

The login and ATProto protocol split is:

- `username.divine.video`
  Public handle host and user-facing profile surface.
- `login.divine.video`
  Human console for claim, consent, recovery, and lifecycle.
- `entryway.divine.video`
  Shared ATProto Authorization Server for Divine users.
- `pds.divine.video`
  Production PDS host that appears in user DID documents.

## Route Responsibilities

- `POST /api/user/profile`
  Claims or updates `username.divine.video` for NIP-05 only. This must not auto-enable ATProto.
- `POST /api/user/atproto/enable`
  Requires a claimed username, sets `enabled = true`, moves lifecycle to `pending`, and triggers provisioning in `divine-sky`.
- `GET /api/user/atproto/status`
  Returns `enabled`, `state`, `did`, `error`, and `username` for the authenticated user.
- `POST /api/user/atproto/disable`
  Sets `enabled = false`, lifecycle `disabled`, and triggers downstream disable cleanup.

## State Contract

Username claim and ATProto lifecycle are separate:

- after username claim:
  - `atproto_enabled = false`
  - `atproto_state = null`
- after user opt-in:
  - `atproto_enabled = true`
  - `atproto_state = "pending"`
- after provisioning succeeds:
  - `atproto_did = "did:plc:..."`
  - `atproto_state = "ready"`
- after provisioning fails:
  - `atproto_state = "failed"`
  - `atproto_error = "..."`
- after user disables:
  - `atproto_enabled = false`
  - `atproto_state = "disabled"`

`did:plc` is the user identity once provisioning is ready.

## Auth Assumptions

- Username claim and `/api/user/atproto/*` routes sit behind DiVine-authenticated user sessions.
- `divine-sky` service-to-service calls from keycast use bearer-token auth, not user auth.
- `/.well-known/atproto-did` is public, host-based, and served by `divine-router`, not by keycast.

## Operational Boundary

`login.divine.video` is a consent and lifecycle owner, not a PDS, not the public read model, and not the ATProto authorization server:

- It owns whether the user has opted in.
- It decides when provisioning should start or stop.
- It never mints DIDs itself.
- It never serves public DID resolution itself.
- It never serves ATProto discovery metadata itself.

The downstream split is:

- `divine-sky`: provisions `did:plc`, creates PDS accounts, stores durable bridge state
- `divine-name-server`: publishes the public username read model
- `divine-router`: serves read-only `/.well-known/atproto-did`
- `pds.divine.video`: serves the PDS read surface and protected-resource metadata
- `entryway.divine.video`: serves the ATProto authorization-server metadata and token flow

## Runtime Handoff

When a link reaches `ready`, the bridge runtime consumes the shared lifecycle state through `account_links`. Publishing is allowed only when:

- `crosspost_enabled == true`
- `provisioning_state == "ready"`
- `disabled_at IS NULL`

Disabling must:

- stop future mirroring
- remove public DID resolution via the name-server/router read model

For launch, treat the flow as:

- keycast writes consent and lifecycle state
- divine-sky provisions and persists durable bridge state
- divine-name-server publishes public handle state
- divine-router resolves `/.well-known/atproto-did` only for active + ready users
- divine-atbridge publishes only for opted-in + ready users
- `pds.divine.video` remains the host clients discover after handle resolution
- `entryway.divine.video` remains the authorization server clients discover through protected-resource metadata

`divine-handle-gateway` also self-heals persisted lifecycle state on startup:

- it replays `pending` rows through provisioning
- it republishes existing `ready`, `failed`, and `disabled` rows to keycast and `divine-name-server`
- this repairs stale public handle resolution after older deploys or manual provisioning paths wrote `account_links` without updating the public read model
