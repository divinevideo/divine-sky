# login.divine.video

## Purpose

`login.divine.video` is the authenticated control plane for DiVine account linking. It owns username claim state, ATProto consent, ATProto lifecycle state, the user-facing enable/status/disable API, and the delegated ATProto Authorization Server surface used by external Bluesky-compatible clients.

It does not serve `/.well-known/atproto-did`. That read-only host resolution now belongs to `divine-router`, which reads the public state published by `divine-name-server`.

## Phase 1 User Surface

Phase 1 ships the user-visible ATProto controls inside the existing advanced settings page at `settings/security`.

The page exposes a `Bluesky Account` card for authenticated cookie-session users:

- no username claimed:
  - show username entry and `Claim username`
- username claimed, Bluesky disabled:
  - show `username.divine.video`
  - show `Enable Bluesky account`
- pending:
  - show provisioning progress and keep polling keycast status
- ready:
  - show `@username.divine.video`
  - show the resolved `did:plc:...`
  - explain that public DID resolution and future cross-posting are active
- failed:
  - show the last provisioning error from keycast
  - allow retry via `Enable Bluesky account`
- disabled:
  - explain that public DID resolution and future cross-posting are off
  - allow re-enable with the existing username

The card is visible without the password-unlock flow that protects private-key export. Email verification requirements still follow the existing security settings page rules.

## Route Responsibilities

- `GET /api/user/profile`
  Returns the claimed username source of truth for the authenticated user.
- `POST /api/user/profile`
  Claims or updates `username.divine.video` for NIP-05 only. This must not auto-enable ATProto.
- `POST /api/user/atproto/enable`
  Requires a claimed username, sets `enabled = true`, moves lifecycle to `pending`, and triggers provisioning in `divine-sky`.
- `GET /api/user/atproto/status`
  Returns `enabled`, `state`, `did`, `error`, and `username` for the authenticated user.
- `POST /api/user/atproto/disable`
  Sets `enabled = false`, lifecycle `disabled`, triggers downstream disable cleanup, and revokes active delegated ATProto OAuth refresh sessions for that account.
- `GET /.well-known/oauth-authorization-server`
  Publishes ATProto Authorization Server metadata for delegated auth discovery.
- `POST /api/atproto/oauth/par`
  Accepts ATProto PAR requests for `scope=atproto`, requires an initial DPoP proof, stores dedicated auth-server session state, and issues a `DPoP-Nonce` header for the session.
- `GET /api/atproto/oauth/authorize`
  Reuses the existing DiVine browser session, requires a `ready` account link, and returns an authorization code to the client.
- `POST /api/atproto/oauth/token`
  Exchanges the authorization code or refresh token for DPoP-bound ATProto tokens, rotates the session nonce, and binds the issued access token to the session `cnf.jkt`.

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

External app login is only allowed when:

- `atproto_enabled = true`
- `atproto_state = "ready"`
- `atproto_did IS NOT NULL`

## Auth Assumptions

- Username claim and `/api/user/atproto/*` routes sit behind DiVine-authenticated user sessions.
- `divine-sky` service-to-service calls from keycast use bearer-token auth, not user auth.
- `/.well-known/atproto-did` is public, host-based, and served by `divine-router`, not by keycast.
- External Bluesky-compatible clients discover `login.divine.video` through the PDS `/.well-known/oauth-protected-resource` document, not through keycast-specific UI flows.

## ATProto Token Contract

The delegated auth-server flow is intentionally separate from the older Nostr/UCAN OAuth surface.

- ATProto auth sessions live in the `atproto_oauth_sessions` table, not the generic OAuth tables.
- Public clients authenticate with PKCE plus DPoP and use `token_endpoint_auth_method = none`.
- Confidential clients use an HTTPS `client_id` metadata document plus `private_key_jwt` client assertions at PAR, authorization-code token exchange, and refresh, with `iss = sub = client_id` and `aud = <authorization-server issuer>`.
- Keycast resolves client metadata at PAR time and validates `client_id`, `redirect_uris`, `token_endpoint_auth_method`, and signing keys from `jwks` or `jwks_uri` before creating the session.
- `POST /api/atproto/oauth/token` issues ES256K JWT access tokens with:
  - `iss = https://login.divine.video`
  - `aud = <configured PDS DID>`
  - `sub = <ready user did:plc>`
  - `scope = com.atproto.access`
- `POST /api/atproto/oauth/token` also returns opaque refresh tokens that are rotated on every successful refresh exchange.
- Access tokens include `cnf.jkt`, which is the RFC 7638 SHA-256 base64url thumbprint of the DPoP public JWK, so `rsky-pds` can enforce proof-of-possession locally.
- Keycast stores refresh-session metadata separately so revocation state does not share tables with bunker/NIP-46 authorizations.
- DPoP is initiated at PAR, enforced again on authorization-code and refresh-token exchanges, and the client must echo the latest `DPoP-Nonce` returned by the server for the next DPoP-bound request in that session.
- Confidential-client sessions are also bound to the client-authentication key established at PAR; key rotation must not silently change the active session key.

Operationally, that means disable actions block new delegated approvals immediately and revoke refresh capability right away, but already-issued access tokens remain usable until their short expiry window closes.

## Operational Boundary

`login.divine.video` is a consent and lifecycle owner, not a PDS and not the public read model:

- It owns whether the user has opted in.
- It decides when provisioning should start or stop.
- It never mints DIDs itself.
- It never serves public DID resolution itself.

The downstream split is:

- `divine-sky`: provisions `did:plc`, creates PDS accounts, stores durable bridge state
- `divine-name-server`: publishes the public username read model
- `divine-router`: serves read-only `/.well-known/atproto-did`
- `rsky-pds`: acts as the protected resource, advertises `authorization_servers`, and verifies ES256K access tokens from the configured auth server origin

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
- rsky-pds publishes `/.well-known/oauth-protected-resource` and trusts the configured auth-server signing key
- divine-atbridge publishes only for opted-in + ready users

From the user perspective, `ready` means all of the following are true:

- keycast shows `enabled = true` and `state = ready`
- the user can see a `did:plc:...` on `settings/security`
- `divine-name-server` has published the public handle state
- `divine-router` can serve `/.well-known/atproto-did`
- future Nostr video publishes are eligible for mirroring because the bridge sees `crosspost_enabled && ready`

`divine-handle-gateway` also self-heals persisted lifecycle state on startup:

- it replays `pending` rows through provisioning
- it republishes existing `ready`, `failed`, and `disabled` rows to keycast and `divine-name-server`
- this repairs stale public handle resolution after older deploys or manual provisioning paths wrote `account_links` without updating the public read model
