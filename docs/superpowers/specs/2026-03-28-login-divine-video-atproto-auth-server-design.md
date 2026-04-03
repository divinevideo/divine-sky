# login.divine.video ATProto Auth Server Design

> **Superseded by:** [entryway.divine.video ATProto Login Boundary Design](/Users/rabble/code/divine/divine-sky/docs/superpowers/specs/2026-04-03-entryway-divine-video-atproto-login-boundary-design.md)
>
> This document reflects the earlier assumption that `login.divine.video` would be the public ATProto Authorization Server. The current boundary is `login.divine.video` for the Keycast human console and `entryway.divine.video` for the public ATProto Authorization Server.

## Goal

Make `login.divine.video` a valid ATProto Authorization Server so other Bluesky-compatible clients can use it for account authentication.

This phase is separate from self-serve Bluesky enablement. It starts only after a user already has a valid, `ready` ATProto account link.

## Why This Is Separate

Phase 1 is mostly product/UI work over an existing provisioning contract.

Phase 2 is a protocol-facing system:

- external Bluesky apps need standards-compliant discovery
- the PDS must advertise an Authorization Server origin
- the Authorization Server must implement ATProto OAuth metadata and flow requirements
- token semantics must be valid for ATProto clients, not just for DiVine's existing Nostr/UCAN flows

Bundling this with Phase 1 would blur two different responsibilities and make both harder to ship cleanly.

## Current State

There are good building blocks, but not an ATProto Authorization Server yet.

### In keycast

- Keycast already has a mature human authentication and consent surface on `login.divine.video`.
- It already implements a generic OAuth flow for third-party apps in `../keycast/api/src/api/http/oauth.rs` and documents those routes in `../keycast/CLAUDE.md`.
- That existing flow issues UCAN access tokens plus NIP-46 bunker URLs for Nostr use cases. It is not the ATProto OAuth profile.
- There are no ATProto OAuth discovery endpoints such as `/.well-known/oauth-protected-resource` or `/.well-known/oauth-authorization-server` in the current keycast codebase.

### In rsky-pds

- `rsky-pds` already owns the ATProto PDS identity boundary: `com.atproto.server.describeServer`, service DID config, and `/.well-known/did.json` in `../rsky/rsky-pds/src/apis/com/atproto/server/describe_server.rs` and `../rsky/rsky-pds/src/well_known.rs`.
- `rsky-pds` also owns ATProto access and refresh token semantics in `../rsky/rsky-pds/src/auth_verifier.rs`.
- The code already has an `entryway` concept in auth verification, but there is no implemented ATProto OAuth Authorization Server surface today.

### In the ATProto spec

The official OAuth spec explicitly allows the PDS ("Resource Server") to point at a separate Authorization Server origin, for example an entryway service, via:

- `/.well-known/oauth-protected-resource` on the PDS
- `/.well-known/oauth-authorization-server` on the Authorization Server

It also requires:

- `atproto` scope support
- PKCE for all clients
- PAR support
- DPoP on token and resource requests
- a real browser-facing authorization interface

Source: official AT Protocol OAuth specification at https://atproto.com/specs/oauth.

## Architecture Options

### Option 1: Reuse keycast OAuth directly

Use the existing keycast `/api/oauth/authorize` and `/api/oauth/token` flow as the ATProto auth server.

Pros:

- reuses existing login and consent UI
- keeps everything under `login.divine.video`

Cons:

- current token model is UCAN plus bunker URL, not ATProto OAuth tokens
- current endpoint and metadata surface is not spec-compliant for ATProto clients
- high risk of breaking the existing Nostr client OAuth behavior while retrofitting protocol semantics

### Option 2: Add a dedicated ATProto auth-server module at `login.divine.video`

Keep keycast as the human-facing login/consent product, but add a separate ATProto Authorization Server surface with its own endpoints, metadata, token issuance, and session storage.

Pros:

- preserves the `login.divine.video` product boundary the user wants
- keeps ATProto semantics separate from existing Nostr/UCAN OAuth
- lets `rsky-pds` remain the resource server while delegating auth to `login.divine.video`

Cons:

- more up-front protocol work
- needs PDS metadata and token verification changes

### Option 3: Keep auth on the PDS and use `login.divine.video` only as a first-party UI

Pros:

- smallest protocol delta

Cons:

- does not satisfy the requirement that `login.divine.video` be the valid auth server used by other Bluesky apps

## Recommendation

Choose Option 2.

`login.divine.video` should become a dedicated ATProto Authorization Server surface, but not by pretending the current keycast OAuth flow is already ATProto-compliant.

Concretely:

- keep `rsky-pds` as the PDS and protected resource
- add a dedicated ATProto auth-server module in keycast, on the same `login.divine.video` origin
- teach `rsky-pds` to publish protected-resource metadata pointing to `https://login.divine.video`
- teach `rsky-pds` to trust tokens issued by that auth server
- keep human authentication and consent anchored in the existing login product

## Phase 2 Design

### 1. Discovery and metadata

`rsky-pds` must publish:

- `/.well-known/oauth-protected-resource`
- an `authorization_servers` array containing the `login.divine.video` origin

`login.divine.video` must publish:

- `/.well-known/oauth-authorization-server`
- ATProto-required metadata fields including:
  - `issuer`
  - `authorization_endpoint`
  - `token_endpoint`
  - `pushed_authorization_request_endpoint`
  - `scopes_supported` including `atproto`
  - `authorization_response_iss_parameter_supported=true`
  - `require_pushed_authorization_requests=true`
  - DPoP signing metadata

### 2. Authorization interface

The browser UI should reuse the existing `login.divine.video` account session model.

Flow:

1. External client discovers the user's PDS and auth server.
2. Client sends PAR to `login.divine.video`.
3. Browser is redirected to the ATProto authorization UI on `login.divine.video`.
4. User authenticates with the existing DiVine account session if needed.
5. User approves or rejects scopes for the already-linked ATProto account.
6. Auth server returns code and later tokens to the client.

Important constraint:

- only users with a `ready` ATProto account link should be allowed to complete this flow

### 3. Session and token model

Do not reuse UCAN access tokens for ATProto clients.

Instead:

- add dedicated ATProto OAuth session storage
- issue ATProto-compliant access and refresh tokens
- bind tokens to DPoP
- keep session revocation separate from the Nostr bunker/UCAN authorization tables

This keeps the existing Nostr product intact and makes protocol validation simpler.

### 4. PDS integration

`rsky-pds` must accept and validate tokens from the external Authorization Server and enforce the usual ATProto resource semantics against the repo DID that belongs to the logged-in account.

That means Phase 2 includes both:

- auth-server work in keycast
- trust and discovery work in `rsky-pds`

## Product Rules

- No ATProto external-app login unless the account link is `ready`.
- A user can still have a DiVine login session without an ATProto-ready account.
- The ATProto consent screen should clearly distinguish:
  - account authentication only (`atproto` scope)
  - broader write scopes if/when supported later
- The first implementation should prefer authentication-only or minimal transitional scopes over full write scope breadth.

## Non-Goals

- replacing Phase 1 lifecycle UI
- migrating existing Nostr OAuth clients to ATProto
- shipping advanced scope bundles in the same milestone
- unifying UCAN and ATProto token stores
- changing PDS account hosting or DID provisioning

## Risks

### Protocol surface is bigger than the current repo seams

Keycast currently owns human login plus Nostr OAuth. `rsky-pds` owns ATProto token and service semantics. Phase 2 crosses that boundary on purpose and will need an explicit contract, not ad hoc endpoint additions.

### Existing keycast OAuth is similar but not interchangeable

The similarity is a trap. Reusing the generic OAuth implementation without a separate ATProto token and metadata model will likely create an almost-correct server that real Bluesky clients reject.

### PDS support is incomplete today

The current `rsky-pds` codebase shows signs of future entryway support, but there is no complete external Authorization Server implementation yet. This is real new protocol work.

### Revocation and cache behavior

The Bluesky team's own deployment notes show that delegated auth-server designs can have revocation lag if access tokens remain valid briefly. Session and revocation behavior should be called out explicitly in the UX and ops docs.

## Acceptance Criteria

- A Bluesky-compatible client can discover the user's PDS and the `login.divine.video` Authorization Server through official metadata endpoints.
- The client can complete a standards-compliant ATProto OAuth flow against `login.divine.video`.
- The token response identifies the user's DID correctly.
- The client can use the returned access token to access the PDS as that account.
- Revoking the session from DiVine removes the client's ability to refresh and eventually use the session.
