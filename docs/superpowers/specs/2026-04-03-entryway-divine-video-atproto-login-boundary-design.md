# entryway.divine.video ATProto Login Boundary Design

## Goal

Let users sign in to third-party Bluesky-compatible clients with a `username.divine.video` account without turning `login.divine.video` into the public ATProto protocol origin.

The public protocol chain should be:

- `username.divine.video` as the handle users type into clients
- `pds.divine.video` as the protected resource / PDS host
- `entryway.divine.video` as the ATProto Authorization Server
- `login.divine.video` as the human-facing Keycast console for DiVine + Nostr auth, consent, recovery, and lifecycle state

## Why This Supersedes Older Phase 2 Docs

The older Phase 2 auth-server spec assumed `login.divine.video` itself should become the ATProto Authorization Server.

That is no longer the cleanest boundary, and the repository already reflects that:

- [login-divine-video.md](/Users/rabble/code/divine/divine-sky/docs/runbooks/login-divine-video.md) says `login.divine.video` is not the ATProto authorization server
- [divine-multi-user-atproto-login-design.md](/Users/rabble/code/divine/divine-sky/docs/superpowers/specs/2026-03-29-divine-multi-user-atproto-login-design.md) recommends `entryway.divine.video`
- [smoke-divine-atproto-login.sh](/Users/rabble/code/divine/divine-sky/scripts/smoke-divine-atproto-login.sh) already verifies `entryway.divine.video`
- `keycast` ATProto OAuth code already has an `ATPROTO_ENTRYWAY_ORIGIN` concept and defaults to `https://entryway.divine.video`

So the remaining work is not "should we use `entryway`?" It is "normalize the contract everywhere so the code, tests, docs, and rollout all describe the same boundary."

## Recommendation

Use `entryway.divine.video` as the public ATProto Authorization Server.

Keep `login.divine.video` as Keycast's human-facing surface.

This is the right split because:

- it keeps DiVine + Nostr product UX on the Keycast host users already know
- it preserves a clean ATProto protocol hostname for discovery and OAuth
- it matches ATProto discovery rules better than mixing human-console and protocol roles on the same public host
- it lets `entryway` be implemented either as a dedicated service or as host-aware routing on top of Keycast, without changing the public contract

## Host Responsibilities

### `username.divine.video`

- Public handle host
- Used for NIP-05 / ATProto handle resolution
- Must resolve to the user's DID only when the account is active and `ready`

### `login.divine.video`

- Keycast human console
- DiVine + Nostr login UX
- Username claim and recovery UX
- ATProto enable / disable / status lifecycle UX
- Consent and lifecycle source of truth

It must not be treated as the public ATProto Authorization Server in external client docs or smoke tests.

### `entryway.divine.video`

- Public ATProto Authorization Server
- Serves `/.well-known/oauth-authorization-server`
- Owns PAR, authorization, token, refresh, and DPoP-facing OAuth behavior
- Can reuse Keycast sessions and lifecycle state behind the scenes

Whether `entryway` is a separate service or host-aware behavior on the same Keycast deployment is an implementation detail. Publicly, it is a distinct ATProto protocol origin.

### `pds.divine.video`

- Public PDS and protected resource
- Serves `/.well-known/oauth-protected-resource`
- Accepts ATProto access tokens minted by `entryway.divine.video`

## End-To-End Login Flow

1. A user claims `username.divine.video` in Keycast.
2. The user enables ATProto from `login.divine.video`.
3. `divine-sky` provisions the account until lifecycle is `ready`.
4. A Bluesky-compatible client receives `username.divine.video` from the user.
5. Handle resolution leads the client to the user's DID and `pds.divine.video`.
6. The client fetches `https://pds.divine.video/.well-known/oauth-protected-resource`.
7. That metadata advertises `https://entryway.divine.video` as the authorization server.
8. The client fetches `https://entryway.divine.video/.well-known/oauth-authorization-server`.
9. The client runs PAR plus browser auth plus token exchange against `entryway.divine.video`.
10. `entryway` consults Keycast-owned lifecycle state and only authorizes users whose ATProto account is `ready`.
11. The returned access token works against `pds.divine.video`.
12. Disabling from `login.divine.video` blocks new approvals and refresh immediately; existing short-lived access tokens expire naturally.

## Product Rules

- No external ATProto OAuth login unless lifecycle is `ready`.
- `login.divine.video` remains the place users manage consent and account state.
- External ATProto clients should never need to know about DiVine's internal service boundaries beyond `username.divine.video`, `pds.divine.video`, and `entryway.divine.video`.
- The first-party DiVine/Nostr product may continue to use `login.divine.video` directly for its own auth flows.

## Implementation Consequences

### Keycast

- Keep lifecycle, consent, and user session ownership in Keycast.
- Make host-aware ATProto auth-server behavior explicit: `entryway` is allowed to serve auth-server metadata and ATProto OAuth endpoints; `login` is not the public ATProto contract.
- Tighten tests so `entryway` hostnames are the expected issuer and endpoint origins.

### `rsky-pds`

- Protected-resource metadata must advertise `https://entryway.divine.video`.
- Token trust config must continue to point at the entryway issuer and public key.

### Docs and Runbooks

- Older docs that say "`login.divine.video` is the Authorization Server" should be marked superseded or updated.
- Smoke tests and launch docs should treat `entryway.divine.video` as the protocol hostname and `login.divine.video` as the human console.

## Non-Goals

- Replacing `login.divine.video` for DiVine or Nostr UX
- Making `login.divine.video` a PDS
- Replacing `pds.divine.video` as the protected resource
- Collapsing all hostnames into a single public origin

## Acceptance Criteria

- A user can enter `username.divine.video` into a Bluesky-compatible client and be taken through a discovery chain that leads to `pds.divine.video` plus `entryway.divine.video`.
- `entryway.divine.video` is the only ATProto Authorization Server origin advertised in protected-resource metadata.
- `login.divine.video` remains the human-facing Keycast console and no longer appears as the public ATProto Authorization Server in current Phase 2 docs.
- Smoke tests, runbooks, config examples, and protocol tests all describe the same host boundary.
- The implementation still enforces `ready` lifecycle gating and refresh-cutoff-on-disable semantics.
