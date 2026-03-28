# DiVine Multi-User ATProto Login Design

## Goal

Make every active `username.divine.video` account a real ATProto identity that can be used to sign in to Bluesky-compatible clients from the handle alone, without the user knowing a PDS hostname.

The concrete acceptance target is:

- a user enters `rabble.divine.video` in a Bluesky-style login flow
- the client resolves the handle to the correct DID
- the DID document points at the Divine production PDS
- the PDS publishes valid ATProto resource metadata
- the client discovers the Divine Authorization Server or entryway
- the login completes successfully for a real multi-user Divine account

## Current State On March 29, 2026

The live production and staging surfaces show that DiVine is partway there, but the login chain is not complete:

- `https://public.api.bsky.app/xrpc/com.atproto.identity.resolveHandle?handle=rabble.divine.video` returns `did:plc:zd3nytjalouy7dv22w2om5hu`
- `https://rabble.divine.video/.well-known/atproto-did` returns the same DID
- the PLC document for that DID points to `https://pds.staging.dvines.org` as the `AtprotoPersonalDataServer`
- `https://pds.staging.dvines.org/xrpc/com.atproto.server.describeServer` returns JSON, but `https://pds.staging.dvines.org/.well-known/oauth-protected-resource` returns `500`
- `https://login.divine.video/.well-known/oauth-authorization-server` returns the DiVine Login HTML shell instead of JSON metadata
- `https://pds.divine.video/xrpc/com.atproto.server.describeServer` currently returns the public DiVine web HTML shell instead of PDS JSON

Conclusion:

- public handle resolution for `*.divine.video` already exists
- the live DID for at least one canary user still points at staging infrastructure
- the production PDS hostname is not yet serving the PDS protocol surface
- the ATProto auth discovery chain is broken

## Approaches Considered

### 1. Shared Divine entryway or auth service plus shared Divine PDS

Keep `pds.divine.video` as the repo host in user DID documents. Add a single Divine Authorization Server or entryway for all `*.divine.video` users. Keep `login.divine.video` as the human account console and reuse its backend state where useful, but do not make it the public ATProto protocol origin.

Pros:

- clean protocol boundary
- aligns with ATProto handle-first login
- supports all Divine users behind one shared auth surface
- keeps `rsky-pds` or other Blacksky-compatible PDS infrastructure in the center

Cons:

- more moving pieces than a PDS-only shortcut
- requires a real authorization server and resource-metadata implementation

### 2. Turn `login.divine.video` into the ATProto Authorization Server directly

Extend the existing Keycast-backed login stack so `login.divine.video` owns the ATProto auth server metadata and endpoints directly.

Pros:

- reuses current account and consent UI
- fewer public hostnames

Cons:

- muddies the boundary between human-facing Divine login UX and ATProto protocol surfaces
- current live behavior already shows this hostname accidentally impersonating unrelated ATProto paths
- harder to reason about long-term than a dedicated entryway or auth origin

### 3. PDS-only login first, defer shared auth or entryway

Rely on `rsky-pds` legacy session endpoints first and postpone ATProto OAuth discovery.

Pros:

- fastest path to basic compatibility with older clients
- smaller first implementation

Cons:

- not enough for the long-term ATProto OAuth direction
- does not solve the real handle-first authorization-server discovery problem
- risks another migration later

### Recommendation

Approach 1 is the right long-term shape.

Use a shared Divine Authorization Server or entryway for all `*.divine.video` users, keep `pds.divine.video` as the PDS host that appears in DID documents, and keep `login.divine.video` as the DiVine account console. This preserves the existing control-plane boundary, matches ATProto discovery rules, and keeps the system compatible with Blacksky and third-party infrastructure instead of forcing the official Bluesky stack.

## Recommended Architecture

### Domain Ownership

- `username.divine.video`
  Public identity hostname only. `divine-router` serves `/.well-known/atproto-did` and normal Divine profile routing.
- `divine-name-server`
  Owns the public username read model, reservation logic, and ATProto readiness state replicated to the router edge.
- `pds.divine.video`
  Multi-user Divine PDS cluster. This is the `AtprotoPersonalDataServer` endpoint recorded in every user DID document.
- `entryway.divine.video`
  Shared Authorization Server and optional virtual PDS entryway for all Divine ATProto users. Clients discover this from `pds.divine.video/.well-known/oauth-protected-resource`.
- `login.divine.video`
  Human-facing Divine account console, consent, claim, recovery, and ATProto lifecycle UI. It may share application code and session state with the entryway, but it does not own the ATProto well-known discovery endpoints.

### Identity And Login Flow

1. A user enters `username.divine.video` in a client login flow.
2. The client resolves the handle to a DID and verifies it bidirectionally against the DID document.
3. The DID document declares `https://pds.divine.video` as the PDS service endpoint.
4. The client fetches `https://pds.divine.video/.well-known/oauth-protected-resource`.
5. That document advertises `https://entryway.divine.video` as the sole authorization server.
6. The client fetches `https://entryway.divine.video/.well-known/oauth-authorization-server`.
7. The client uses PAR, PKCE, and DPoP against the entryway and completes the approval flow.
8. The entryway returns tokens for the account DID and the client begins using `pds.divine.video` as the resource server.

### Compatibility Rule

Do not regress the legacy session surface while shipping this.

`rsky-pds` already exposes `com.atproto.server.createSession`, `getSession`, `refreshSession`, and `deleteSession`. Those endpoints should keep working on `pds.divine.video` for current clients while Divine adds the OAuth-complete path needed for the broader ATProto future.

The production target is therefore:

- legacy session login works against `pds.divine.video`
- OAuth discovery and login works through `pds.divine.video` plus `entryway.divine.video`
- users still enter only their handle

## Multi-User Account Model

### Provisioning

Every opted-in Divine user gets:

- a unique `did:plc`
- a stable `username.divine.video` handle
- a per-account repo signing key under Divine custody
- a PLC rotation key under stricter recovery controls
- a DID document whose `AtprotoPersonalDataServer` service endpoint is `https://pds.divine.video`

### Control Plane

Keep the existing split:

- `keycast` or the Divine login stack owns username claim, human authentication, consent, and lifecycle state
- `divine-handle-gateway` and `divine-atbridge` own provisioning, replay, and reconciliation
- `divine-name-server` publishes active handle state
- `divine-router` exposes public handle resolution

The control plane should grow a small amount of extra state for operability:

- current PDS host
- current auth or entryway host
- last PLC sync timestamp
- login capability state such as `session_ready`, `oauth_ready`, or `migrating`

## Migration And Cutover

The live `rabble.divine.video` DID doc still points to `https://pds.staging.dvines.org`. That cannot remain true if Divine wants a real production login story.

The migration has to do three things:

1. New accounts must provision directly onto `https://pds.divine.video`.
2. Existing active Divine accounts must have their PLC service endpoint rotated from staging to production.
3. Public handle resolution must continue uninterrupted during the cutover.

This implies a staged rollout:

- stand up `pds.divine.video` as a real PDS before touching PLC documents
- add protected-resource metadata on the production PDS
- stand up the shared Divine entryway or auth service
- canary-migrate `rabble.divine.video`
- backfill all active `*.divine.video` accounts

## Failure Modes

- Handle resolves, but DID doc still points at staging
  Result: client talks to the wrong PDS or mixed environments.
- DID doc points at production PDS, but PDS metadata is missing or malformed
  Result: OAuth clients cannot discover the authorization server.
- PDS metadata points at the entryway, but entryway metadata is wrong or incomplete
  Result: clients fail before authorization starts.
- Entryway authenticates the user but returns a DID or issuer mismatch
  Result: standards-compliant clients reject the login.
- Login works for one canary user but new provisioning still writes staging endpoints
  Result: rollout appears healthy while new accounts are broken.

## Validation Strategy

### Contract Checks

For any active Divine ATProto account:

- `public.api.bsky.app` resolves the handle to the expected DID
- `https://username.divine.video/.well-known/atproto-did` returns the same DID
- the PLC document points to `https://pds.divine.video`
- `https://pds.divine.video/xrpc/com.atproto.server.describeServer` returns JSON
- `https://pds.divine.video/.well-known/oauth-protected-resource` returns `200 application/json`
- `https://entryway.divine.video/.well-known/oauth-authorization-server` returns `200 application/json`

### End-To-End Checks

- legacy session login succeeds against `pds.divine.video`
- handle-first OAuth login succeeds through the entryway
- a newly provisioned Divine user and a migrated existing Divine user both pass the same checks

## Non-Goals

- replacing Nostr as DiVine's main authoring model
- requiring the official Bluesky PDS or entryway stack
- making `login.divine.video` the PDS
- relying on staging hosts for production user identity

## Source Notes

This design is grounded in:

- the current Divine runbooks and unified ATProto plan in this repo
- live checks against `login.divine.video`, `rabble.divine.video`, `pds.staging.dvines.org`, `pds.divine.video`, and `public.api.bsky.app` on March 29, 2026
- the AT Protocol OAuth spec and Bluesky entryway guidance
