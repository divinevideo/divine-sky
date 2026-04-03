# login.divine.video ATProto Auth Server Implementation Plan

> **Superseded by:** [entryway.divine.video Phase 2 Normalization Plan](/Users/rabble/code/divine/divine-sky/docs/superpowers/plans/2026-04-03-entryway-divine-video-phase2-normalization.md)
>
> This plan reflects the earlier `login.divine.video` auth-server assumption. Execute the entryway-normalized plan instead: `login.divine.video` is the Keycast human console, and `entryway.divine.video` is the public ATProto Authorization Server.

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `login.divine.video` an ATProto Authorization Server that external Bluesky-compatible clients can use for account authentication against DiVine-hosted ATProto accounts.

**Architecture:** Add a dedicated ATProto OAuth/Auth Server surface to keycast instead of overloading the existing UCAN/Nostr OAuth endpoints. Keep `rsky-pds` as the protected resource/PDS, add required well-known metadata on both sides, and gate successful auth on `ready` account links from the existing provisioning lifecycle.

**Tech Stack:** Keycast Rust API and web login surface, rsky-pds Rust service, ATProto OAuth profile requirements (metadata, PAR, PKCE, DPoP), existing DiVine session/auth flows.

---

## Chunk 0: Create Isolated `keycast` And `rsky` Workspaces

### Task 0: Prepare sibling-repo worktrees before implementation

**Files:**
- Verify only

- [ ] **Step 1: Create the `keycast` worktree**

Run:

```bash
cd /Users/rabble/code/divine/keycast
git worktree add .worktrees/phase2-atproto-auth-server -b feat/phase2-atproto-auth-server
```

- [ ] **Step 2: Create the `rsky` worktree**

Run:

```bash
cd /Users/rabble/code/divine/rsky
git worktree add .worktrees/phase2-atproto-protected-resource -b feat/phase2-atproto-protected-resource
```

- [ ] **Step 3: Verify a focused clean baseline in both repos**

Run:
- `cd /Users/rabble/code/divine/keycast/.worktrees/phase2-atproto-auth-server/api && cargo test --test oauth_unit_test -- --nocapture`
- `cd /Users/rabble/code/divine/rsky/.worktrees/phase2-atproto-protected-resource && cargo test -p rsky-pds build_id_resolver_uses_identity_config_timeout_and_cache_ttls -- --nocapture`

Expected: PASS before starting protocol changes.

## Chunk 1: Metadata And Boundary Contract

### Task 1: Add protected-resource metadata to `rsky-pds`

**Files:**
- Create: `../rsky/rsky-pds/src/apis/oauth/protected_resource.rs`
- Modify: `../rsky/rsky-pds/src/lib.rs`
- Modify: `../rsky/rsky-pds/src/well_known.rs`
- Test: `../rsky/rsky-pds/tests/integration_tests.rs`

- [ ] **Step 1: Write a failing integration test for protected-resource metadata**

The test should assert:
- `/.well-known/oauth-protected-resource` exists
- it returns HTTP `200`
- it contains a single `authorization_servers` entry pointing at `https://login.divine.video`

- [ ] **Step 2: Run the focused PDS integration test and verify it fails correctly**

Run: `cd /Users/rabble/code/divine/rsky/.worktrees/phase2-atproto-protected-resource && cargo test -p rsky-pds integration_tests -- --nocapture`

Expected: FAIL because the endpoint does not exist yet.

- [ ] **Step 3: Implement the smallest metadata endpoint and route wiring**

Rules:
- no redirects
- JSON only
- keep the Authorization Server origin configurable by env var

- [ ] **Step 4: Re-run the focused PDS test**

Run: `cd /Users/rabble/code/divine/rsky/.worktrees/phase2-atproto-protected-resource && cargo test -p rsky-pds integration_tests -- --nocapture`

Expected: PASS.

### Task 2: Add Authorization Server metadata to keycast

**Files:**
- Create: `../keycast/api/src/api/http/atproto_oauth_metadata.rs`
- Modify: `../keycast/api/src/api/http/routes.rs`
- Modify: `../keycast/api/openapi.yaml`
- Test: `../keycast/api/tests/atproto_oauth_metadata_test.rs`

- [ ] **Step 1: Write a failing metadata test for `/.well-known/oauth-authorization-server`**

Assert required fields:
- `issuer`
- `authorization_endpoint`
- `token_endpoint`
- `pushed_authorization_request_endpoint`
- `scopes_supported` includes `atproto`
- `require_pushed_authorization_requests` is `true`

- [ ] **Step 2: Run the focused keycast test**

Run: `cd /Users/rabble/code/divine/keycast/.worktrees/phase2-atproto-auth-server/api && cargo test --test atproto_oauth_metadata_test -- --nocapture`

Expected: FAIL because the endpoint does not exist yet.

- [ ] **Step 3: Implement the metadata endpoint with env-driven URLs**

Keep this module ATProto-specific. Do not patch the existing generic OAuth handler to fake metadata.

- [ ] **Step 4: Re-run the focused metadata test**

Run: `cd /Users/rabble/code/divine/keycast/.worktrees/phase2-atproto-auth-server/api && cargo test --test atproto_oauth_metadata_test -- --nocapture`

Expected: PASS.

## Chunk 2: Auth Server Session Model In keycast

### Task 3: Introduce dedicated ATProto OAuth session storage

**Files:**
- Create: `../keycast/database/migrations/YYYYMMDDHHMMSS_add_atproto_oauth_sessions.sql`
- Create: `../keycast/core/src/repositories/atproto_oauth_session.rs`
- Modify: `../keycast/core/src/repositories/mod.rs`
- Test: `../keycast/api/tests/atproto_oauth_session_test.rs`

- [ ] **Step 1: Write a failing repository test for storing and revoking ATProto OAuth sessions**

Cover:
- PAR/request state persistence
- user binding
- DID binding
- refresh-session revocation metadata

- [ ] **Step 2: Run the repository test**

Run: `cd /Users/rabble/code/divine/keycast/.worktrees/phase2-atproto-auth-server/api && cargo test --test atproto_oauth_session_test -- --nocapture`

Expected: FAIL because the storage does not exist.

- [ ] **Step 3: Add the new storage with names clearly separate from existing UCAN/Nostr OAuth tables**

Do not reuse `oauth_authorizations` for ATProto sessions.

- [ ] **Step 4: Re-run the repository test**

Run: `cd /Users/rabble/code/divine/keycast/.worktrees/phase2-atproto-auth-server/api && cargo test --test atproto_oauth_session_test -- --nocapture`

Expected: PASS.

### Task 4: Add ATProto PAR, authorize, and token endpoints in keycast

**Files:**
- Create: `../keycast/api/src/api/http/atproto_oauth.rs`
- Modify: `../keycast/api/src/api/http/routes.rs`
- Modify: `../keycast/web/src/routes/login/+page.svelte`
- Test: `../keycast/api/tests/atproto_oauth_http_test.rs`

- [ ] **Step 1: Write failing HTTP tests for the ATProto auth flow skeleton**

Cover:
- PAR request acceptance
- browser redirect to authorization UI
- login session reuse
- code exchange to token response
- rejection when account link is not `ready`

- [ ] **Step 2: Run the focused HTTP test**

Run: `cd /Users/rabble/code/divine/keycast/.worktrees/phase2-atproto-auth-server/api && cargo test --test atproto_oauth_http_test -- --nocapture`

Expected: FAIL because the endpoints do not exist yet.

- [ ] **Step 3: Implement the minimum auth flow**

Rules:
- reuse DiVine user session auth for the human login step
- require `ready` ATProto state before approval succeeds
- keep ATProto endpoints in a dedicated module
- do not change the existing generic `/api/oauth/*` behavior unless a shared helper extraction is necessary

- [ ] **Step 4: Re-run the focused HTTP test**

Run: `cd /Users/rabble/code/divine/keycast/.worktrees/phase2-atproto-auth-server/api && cargo test --test atproto_oauth_http_test -- --nocapture`

Expected: PASS.

## Chunk 3: PDS Token Trust And Enforcement

### Task 5: Teach `rsky-pds` to trust the external Authorization Server

**Files:**
- Modify: `../rsky/rsky-pds/src/auth_verifier.rs`
- Modify: `../rsky/rsky-pds/src/config/mod.rs`
- Modify: `../rsky/rsky-pds/src/apis/com/atproto/server/describe_server.rs`
- Test: `../rsky/rsky-pds/tests/integration_tests.rs`

- [ ] **Step 1: Write a failing integration test for access-token acceptance from the external auth server**

The test should prove:
- a valid externally issued ATProto token is accepted
- token audience and issuer are checked
- non-ready or revoked sessions are rejected

- [ ] **Step 2: Run the focused PDS test**

Run: `cd /Users/rabble/code/divine/rsky/.worktrees/phase2-atproto-protected-resource && cargo test -p rsky-pds integration_tests -- --nocapture`

Expected: FAIL because external auth-server trust is not implemented.

- [ ] **Step 3: Add the minimum trust configuration and verifier logic**

Prefer explicit issuer/origin config and narrow validation. Do not weaken current token validation to make tests pass.

- [ ] **Step 4: Re-run the focused PDS test**

Run: `cd /Users/rabble/code/divine/rsky/.worktrees/phase2-atproto-protected-resource && cargo test -p rsky-pds integration_tests -- --nocapture`

Expected: PASS.

## Chunk 4: Interop, Revocation, And Docs

### Task 6: Document and verify end-to-end client login

**Files:**
- Modify: `docs/runbooks/login-divine-video.md`
- Modify: `docs/runbooks/launch-checklist.md`
- Create: `docs/runbooks/atproto-auth-server-smoke-test.md`

- [ ] **Step 1: Write the end-to-end smoke flow**

Include:
- user account must already be `ready`
- client discovery from PDS metadata
- PAR
- browser auth + approval
- token exchange
- authenticated PDS call
- revocation and retry failure

- [ ] **Step 2: Run service-level verification**

Run:
- `cd /Users/rabble/code/divine/keycast/.worktrees/phase2-atproto-auth-server/api && cargo test --test atproto_oauth_metadata_test -- --nocapture`
- `cd /Users/rabble/code/divine/keycast/.worktrees/phase2-atproto-auth-server/api && cargo test --test atproto_oauth_http_test -- --nocapture`
- `cd /Users/rabble/code/divine/rsky/.worktrees/phase2-atproto-protected-resource && cargo test -p rsky-pds integration_tests -- --nocapture`

Expected: PASS.

- [ ] **Step 3: Commit the Phase 2 implementation branch**

```bash
git -C /Users/rabble/code/divine/keycast/.worktrees/phase2-atproto-auth-server add api/src/api/http/atproto_oauth_metadata.rs api/src/api/http/atproto_oauth.rs api/src/api/http/routes.rs api/tests/atproto_oauth_metadata_test.rs api/tests/atproto_oauth_http_test.rs database/migrations core/src/repositories/atproto_oauth_session.rs core/src/repositories/mod.rs web/src/routes/login/+page.svelte
git -C /Users/rabble/code/divine/rsky/.worktrees/phase2-atproto-protected-resource add rsky-pds/src/apis/oauth/protected_resource.rs rsky-pds/src/lib.rs rsky-pds/src/well_known.rs rsky-pds/src/auth_verifier.rs rsky-pds/src/config/mod.rs rsky-pds/src/apis/com/atproto/server/describe_server.rs rsky-pds/tests/integration_tests.rs
git -C /Users/rabble/code/divine/divine-sky/.worktrees/plan-phase2-atproto-auth-server add docs/runbooks/login-divine-video.md docs/runbooks/launch-checklist.md docs/runbooks/atproto-auth-server-smoke-test.md
git -C /Users/rabble/code/divine/keycast/.worktrees/phase2-atproto-auth-server commit -m "feat: add atproto authorization server surface"
git -C /Users/rabble/code/divine/rsky/.worktrees/phase2-atproto-protected-resource commit -m "feat: trust external atproto authorization server"
```
