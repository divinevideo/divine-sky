# DiVine Multi-User ATProto Login Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every active `username.divine.video` account a real ATProto identity that can sign into Bluesky-compatible clients from the handle alone, with `pds.divine.video` as the production repo host and a shared Divine authorization or entryway service for all users.

**Architecture:** Keep public handle resolution on `divine-router` plus `divine-name-server`, keep user DID documents pointed at `pds.divine.video`, preserve the legacy PDS session endpoints for current clients, and add the missing OAuth discovery chain by serving protected-resource metadata from the PDS and a real authorization-server metadata plus PAR and token flow from a shared Divine entryway. Keep `login.divine.video` as the human account console; do not make it the public protocol origin.

**Tech Stack:** Rust, `rsky-pds`, `keycast`, Axum, Rocket, Fastly, Cloudflare Workers, PostgreSQL, PLC directory, ATProto OAuth, PKCE, PAR, DPoP

---

## Scope And Boundaries

- This is a cross-repo plan. The implementation spans:
  - `/Users/rabble/code/divine/divine-sky`
  - `/Users/rabble/code/divine/rsky`
  - `/Users/rabble/code/divine/keycast`
  - `/Users/rabble/code/divine/divine-router`
  - `/Users/rabble/code/divine/divine-name-server`
- `login.divine.video` remains a user-facing Divine control plane.
- `entryway.divine.video` is the recommended new protocol hostname for ATProto Authorization Server behavior.
- `pds.divine.video` is the required production PDS hostname that must appear in user DID documents.
- `pds.staging.dvines.org` must stop appearing in production user DID documents.

## File Structure

- Modify: `/Users/rabble/code/divine/divine-sky/docs/runbooks/login-divine-video.md`
  Clarify the split between login UI, entryway, and PDS ownership.
- Modify: `/Users/rabble/code/divine/divine-sky/docs/runbooks/atproto-opt-in-smoke-test.md`
  Replace staging-only assumptions with production login-chain checks.
- Create: `/Users/rabble/code/divine/divine-sky/docs/runbooks/divine-atproto-login-canary.md`
  One end-to-end runbook for handle resolution, PDS metadata, and login verification.
- Create: `/Users/rabble/code/divine/divine-sky/scripts/smoke-divine-atproto-login.sh`
  Multi-host smoke script for `username.divine.video`, `pds.divine.video`, and `entryway.divine.video`.
- Modify: `/Users/rabble/code/divine/divine-router/src/main.rs`
  Keep serving `/.well-known/atproto-did` and reserve the new protocol hostnames from username routing.
- Modify: `/Users/rabble/code/divine/divine-name-server/src/index.ts`
  Ensure the public read model carries the fields needed for login readiness and host cutover.
- Modify: `/Users/rabble/code/divine/divine-name-server/tests/atproto-sync.test.ts`
  Verify login-related fields replicate correctly.
- Modify: `/Users/rabble/code/divine/divine-sky/crates/divine-atbridge/src/provisioner.rs`
  Provision all new users onto `https://pds.divine.video`.
- Modify: `/Users/rabble/code/divine/divine-sky/crates/divine-atbridge/src/plc_directory.rs`
  Add PLC update support for service-endpoint migration.
- Modify: `/Users/rabble/code/divine/divine-sky/crates/divine-atbridge/src/pds_accounts.rs`
  Keep account creation aligned with the production PDS host.
- Create: `/Users/rabble/code/divine/divine-sky/crates/divine-atbridge/src/pds_host_backfill.rs`
  Backfill existing active users from staging PDS endpoints to production.
- Create: `/Users/rabble/code/divine/divine-sky/crates/divine-atbridge/tests/pds_host_backfill.rs`
  Cover the migration logic.
- Modify: `/Users/rabble/code/divine/divine-sky/crates/divine-handle-gateway/src/provision_runner.rs`
  Reconcile migrated PDS host and login-capability state back into the public read model.
- Modify: `/Users/rabble/code/divine/divine-sky/crates/divine-handle-gateway/src/name_server_client.rs`
  Publish the extra login-readiness fields.
- Modify: `/Users/rabble/code/divine/divine-sky/crates/divine-handle-gateway/tests/provision_flow.rs`
  Verify new provisioning and reconciliation on the production PDS host.
- Modify: `/Users/rabble/code/divine/rsky/rsky-pds/src/well_known.rs`
  Add protected-resource metadata and keep well-known routes coherent.
- Modify: `/Users/rabble/code/divine/rsky/rsky-pds/src/config/mod.rs`
  Add configuration for the authorization-server origin exposed by the PDS.
- Create: `/Users/rabble/code/divine/rsky/rsky-pds/tests/oauth_metadata_test.rs`
  Validate the protected-resource metadata shape.
- Modify: `/Users/rabble/code/divine/keycast/api/src/api/http/routes.rs`
  Mount PAR and ATProto auth endpoints for the entryway host.
- Create: `/Users/rabble/code/divine/keycast/api/src/api/http/atproto_oauth.rs`
  Implement authorization-server metadata, PAR, and ATProto token behavior.
- Create: `/Users/rabble/code/divine/keycast/api/tests/atproto_oauth_metadata_test.rs`
  Cover authorization-server metadata.
- Create: `/Users/rabble/code/divine/keycast/api/tests/atproto_par_test.rs`
  Cover PAR behavior.
- Create: `/Users/rabble/code/divine/keycast/api/tests/atproto_dpop_token_test.rs`
  Cover DPoP-bound token requests and issuer or subject consistency.
- Modify: `/Users/rabble/code/divine/keycast/keycast/src/main.rs`
  Add host-aware routing so `entryway.divine.video` serves protocol surfaces and `login.divine.video` stays a control-plane app.
- Modify: `/Users/rabble/code/divine/keycast/docs/DEPLOYMENT.md`
  Add deployment and smoke-check instructions for the new entryway host.

## Chunk 1: Lock The Production Login Contract

### Task 1: Capture the current login-chain contract in docs and smoke checks

**Files:**
- Modify: `/Users/rabble/code/divine/divine-sky/docs/runbooks/login-divine-video.md`
- Modify: `/Users/rabble/code/divine/divine-sky/docs/runbooks/atproto-opt-in-smoke-test.md`
- Create: `/Users/rabble/code/divine/divine-sky/docs/runbooks/divine-atproto-login-canary.md`
- Create: `/Users/rabble/code/divine/divine-sky/scripts/smoke-divine-atproto-login.sh`

- [ ] **Step 1: Write the failing smoke script first**

Create a script that checks:

```bash
curl -fsS "https://public.api.bsky.app/xrpc/com.atproto.identity.resolveHandle?handle=rabble.divine.video"
curl -fsS "https://rabble.divine.video/.well-known/atproto-did"
curl -fsS "https://plc.directory/did:plc:zd3nytjalouy7dv22w2om5hu"
curl -fsS "https://pds.divine.video/xrpc/com.atproto.server.describeServer"
curl -fsS "https://pds.divine.video/.well-known/oauth-protected-resource"
curl -fsS "https://entryway.divine.video/.well-known/oauth-authorization-server"
```

The script should fail if:

- handle resolution and subdomain DID resolution disagree
- the DID document points to any staging hostname
- `pds.divine.video` returns HTML or `404`
- the protected-resource or authorization-server metadata is missing or non-JSON

- [ ] **Step 2: Run the smoke script and confirm it fails on current production**

Run:

```bash
bash /Users/rabble/code/divine/divine-sky/scripts/smoke-divine-atproto-login.sh
```

Expected: FAIL because the live production chain still points at staging and does not serve the PDS and entryway metadata correctly.

- [ ] **Step 3: Update the runbooks to match the new domain split**

Document:

- `login.divine.video` is the human console only
- `entryway.divine.video` is the ATProto Authorization Server
- `pds.divine.video` is the PDS host in DID documents
- `username.divine.video` remains the public handle host

- [ ] **Step 4: Commit the contract docs**

```bash
git -C /Users/rabble/code/divine/divine-sky add \
  docs/runbooks/login-divine-video.md \
  docs/runbooks/atproto-opt-in-smoke-test.md \
  docs/runbooks/divine-atproto-login-canary.md \
  scripts/smoke-divine-atproto-login.sh
git -C /Users/rabble/code/divine/divine-sky commit -m "docs: lock divine atproto login contract"
```

## Chunk 2: Move All Divine Accounts To The Production PDS Host

### Task 2: Make new provisioning write `pds.divine.video`

**Files:**
- Modify: `/Users/rabble/code/divine/divine-sky/crates/divine-atbridge/src/provisioner.rs`
- Modify: `/Users/rabble/code/divine/divine-sky/crates/divine-atbridge/src/pds_accounts.rs`
- Modify: `/Users/rabble/code/divine/divine-sky/crates/divine-handle-gateway/tests/provision_flow.rs`

- [ ] **Step 1: Write the failing provisioning test**

Add a test that provisions `alice.divine.video` and asserts the resulting PLC operation and account creation target `https://pds.divine.video`, not any staging host.

- [ ] **Step 2: Run the focused provisioning test**

Run:

```bash
cargo test -p divine-handle-gateway provision_flow -- --nocapture
```

Expected: FAIL if any production provisioning path still uses staging or an unset PDS host.

- [ ] **Step 3: Implement the minimal production-host fix**

Use the production PDS origin as the only provisioning target for production Divine handles.

- [ ] **Step 4: Re-run the focused provisioning test**

Run:

```bash
cargo test -p divine-handle-gateway provision_flow -- --nocapture
```

Expected: PASS.

### Task 3: Add a backfill path for existing user DID documents

**Files:**
- Modify: `/Users/rabble/code/divine/divine-sky/crates/divine-atbridge/src/plc_directory.rs`
- Create: `/Users/rabble/code/divine/divine-sky/crates/divine-atbridge/src/pds_host_backfill.rs`
- Create: `/Users/rabble/code/divine/divine-sky/crates/divine-atbridge/tests/pds_host_backfill.rs`
- Modify: `/Users/rabble/code/divine/divine-sky/crates/divine-handle-gateway/src/provision_runner.rs`
- Modify: `/Users/rabble/code/divine/divine-sky/crates/divine-handle-gateway/src/name_server_client.rs`

- [ ] **Step 1: Write the failing backfill test**

Cover a stored active user whose PLC document still points at `https://pds.staging.dvines.org`.

Expected behavior:

- PLC service endpoint is updated to `https://pds.divine.video`
- public name-server state is refreshed after the update
- the user never leaves `ready` just because of the host migration

- [ ] **Step 2: Run the focused backfill test**

Run:

```bash
cargo test -p divine-atbridge pds_host_backfill -- --nocapture
```

Expected: FAIL because the backfill flow does not exist yet.

- [ ] **Step 3: Implement the PLC update and reconciliation logic**

Add a dedicated migration path instead of burying this in ad hoc scripts.

- [ ] **Step 4: Re-run the focused backfill test**

Run:

```bash
cargo test -p divine-atbridge pds_host_backfill -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Run the broader Rust verification**

Run:

```bash
cargo test -p divine-atbridge -- --nocapture
cargo test -p divine-handle-gateway -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit the PDS host migration work**

```bash
git -C /Users/rabble/code/divine/divine-sky add \
  crates/divine-atbridge/src/provisioner.rs \
  crates/divine-atbridge/src/pds_accounts.rs \
  crates/divine-atbridge/src/plc_directory.rs \
  crates/divine-atbridge/src/pds_host_backfill.rs \
  crates/divine-atbridge/tests/pds_host_backfill.rs \
  crates/divine-handle-gateway/src/provision_runner.rs \
  crates/divine-handle-gateway/src/name_server_client.rs \
  crates/divine-handle-gateway/tests/provision_flow.rs
git -C /Users/rabble/code/divine/divine-sky commit -m "feat: move divine atproto accounts to production pds host"
```

## Chunk 3: Make `pds.divine.video` A Real Resource Server

### Task 4: Add protected-resource metadata to `rsky-pds`

**Files:**
- Modify: `/Users/rabble/code/divine/rsky/rsky-pds/src/well_known.rs`
- Modify: `/Users/rabble/code/divine/rsky/rsky-pds/src/config/mod.rs`
- Create: `/Users/rabble/code/divine/rsky/rsky-pds/tests/oauth_metadata_test.rs`

- [ ] **Step 1: Write the failing metadata test**

Add a test for:

```http
GET /.well-known/oauth-protected-resource
```

Expected JSON:

```json
{
  "authorization_servers": ["https://entryway.divine.video"]
}
```

Also assert:

- status is `200`
- content type is `application/json`
- the document never redirects

- [ ] **Step 2: Run the focused PDS metadata test**

Run:

```bash
cargo test -p rsky-pds oauth_metadata -- --nocapture
```

Expected: FAIL because the route does not exist or is misconfigured.

- [ ] **Step 3: Implement the metadata route**

Read the authorization-server origin from config and return a strict protected-resource metadata document.

- [ ] **Step 4: Re-run the focused PDS metadata test**

Run:

```bash
cargo test -p rsky-pds oauth_metadata -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Verify existing PDS well-known routes still behave**

Run:

```bash
cargo test -p rsky-pds -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit the PDS metadata work**

```bash
git -C /Users/rabble/code/divine/rsky add \
  rsky-pds/src/well_known.rs \
  rsky-pds/src/config/mod.rs \
  rsky-pds/tests/oauth_metadata_test.rs
git -C /Users/rabble/code/divine/rsky commit -m "feat: add pds protected resource metadata"
```

## Chunk 4: Build The Shared Divine Entryway

### Task 5: Add authorization-server metadata and PAR to the entryway host

**Files:**
- Modify: `/Users/rabble/code/divine/keycast/api/src/api/http/routes.rs`
- Create: `/Users/rabble/code/divine/keycast/api/src/api/http/atproto_oauth.rs`
- Create: `/Users/rabble/code/divine/keycast/api/tests/atproto_oauth_metadata_test.rs`
- Create: `/Users/rabble/code/divine/keycast/api/tests/atproto_par_test.rs`
- Modify: `/Users/rabble/code/divine/keycast/keycast/src/main.rs`

- [ ] **Step 1: Write the failing authorization-server metadata test**

Expected endpoint:

```http
GET /.well-known/oauth-authorization-server
```

Expected fields:

- `issuer = https://entryway.divine.video`
- `authorization_endpoint`
- `token_endpoint`
- `pushed_authorization_request_endpoint`
- `response_types_supported` containing `code`
- `grant_types_supported` containing `authorization_code` and `refresh_token`
- `code_challenge_methods_supported` containing `S256`
- `token_endpoint_auth_methods_supported` containing `none` and `private_key_jwt`
- `scopes_supported` containing `atproto`
- `require_pushed_authorization_requests = true`
- `dpop_signing_alg_values_supported` containing `ES256`

- [ ] **Step 2: Write the failing PAR test**

Require:

- host-aware routing only serves this on `entryway.divine.video`
- PAR rejects malformed client metadata
- PAR persists enough request state for the later authorization step

- [ ] **Step 3: Run the focused entryway tests**

Run:

```bash
cargo test -p keycast-api atproto_oauth_metadata -- --nocapture
cargo test -p keycast-api atproto_par -- --nocapture
```

Expected: FAIL because the entryway protocol surface does not exist yet.

- [ ] **Step 4: Implement metadata plus PAR**

Reuse existing Keycast authentication and consent state where helpful, but keep the ATProto surface host-aware and protocol-correct.

- [ ] **Step 5: Re-run the focused entryway tests**

Run:

```bash
cargo test -p keycast-api atproto_oauth_metadata -- --nocapture
cargo test -p keycast-api atproto_par -- --nocapture
```

Expected: PASS.

### Task 6: Add DPoP-bound token exchange and issuer or subject validation

**Files:**
- Create: `/Users/rabble/code/divine/keycast/api/tests/atproto_dpop_token_test.rs`
- Modify: `/Users/rabble/code/divine/keycast/api/src/api/http/atproto_oauth.rs`

- [ ] **Step 1: Write the failing DPoP token test**

Cover:

- valid DPoP proof yields a token response with `sub = did:plc:...`
- invalid or replayed DPoP proof is rejected
- token responses bind to the expected issuer and account DID

- [ ] **Step 2: Run the focused DPoP token test**

Run:

```bash
cargo test -p keycast-api atproto_dpop_token -- --nocapture
```

Expected: FAIL.

- [ ] **Step 3: Implement minimal DPoP-bound token behavior**

Keep scope narrow:

- only the ATProto-required token flow
- no unrelated Keycast OAuth refactor

- [ ] **Step 4: Re-run the focused DPoP token test**

Run:

```bash
cargo test -p keycast-api atproto_dpop_token -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Run the broader Keycast verification**

Run:

```bash
cargo test -p keycast-api -- --nocapture
cargo check -p keycast --tests
```

Expected: PASS.

- [ ] **Step 6: Commit the entryway work**

```bash
git -C /Users/rabble/code/divine/keycast add \
  api/src/api/http/routes.rs \
  api/src/api/http/atproto_oauth.rs \
  api/tests/atproto_oauth_metadata_test.rs \
  api/tests/atproto_par_test.rs \
  api/tests/atproto_dpop_token_test.rs \
  keycast/src/main.rs \
  docs/DEPLOYMENT.md
git -C /Users/rabble/code/divine/keycast commit -m "feat: add divine atproto entryway"
```

## Chunk 5: Reserve Hostnames And Roll Out Safely

### Task 7: Keep the router and public read model aligned with the new hosts

**Files:**
- Modify: `/Users/rabble/code/divine/divine-router/src/main.rs`
- Modify: `/Users/rabble/code/divine/divine-name-server/src/index.ts`
- Modify: `/Users/rabble/code/divine/divine-name-server/tests/atproto-sync.test.ts`

- [ ] **Step 1: Write the failing host-reservation test**

Assert that `entryway`, `login`, and `pds` remain reserved service hostnames and never become claimable usernames.

- [ ] **Step 2: Run the focused name-server test**

Run:

```bash
cd /Users/rabble/code/divine/divine-name-server && npm test -- atproto-sync.test.ts
```

Expected: FAIL if the reserved-host logic or sync payload is incomplete.

- [ ] **Step 3: Implement the minimal reservation and sync fix**

Make sure:

- router passes through the new service hostnames
- name-server sync keeps the router read model current for migrated accounts

- [ ] **Step 4: Re-run the focused test**

Run:

```bash
cd /Users/rabble/code/divine/divine-name-server && npm test -- atproto-sync.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit the hostname and read-model updates**

```bash
git -C /Users/rabble/code/divine/divine-router add src/main.rs
git -C /Users/rabble/code/divine/divine-router commit -m "fix: reserve divine atproto protocol hosts"

git -C /Users/rabble/code/divine/divine-name-server add src/index.ts tests/atproto-sync.test.ts
git -C /Users/rabble/code/divine/divine-name-server commit -m "feat: sync divine atproto login readiness"
```

## Chunk 6: Canary Migration And Final Verification

### Task 8: Migrate `rabble.divine.video` first, then expand to all active users

**Files:**
- Use the migration code and runbooks from earlier chunks

- [ ] **Step 1: Run the canary migration for `rabble.divine.video`**

Expected result:

- the PLC document now points to `https://pds.divine.video`
- the handle still resolves correctly
- the public read model remains active

- [ ] **Step 2: Run the full smoke script against the canary**

Run:

```bash
bash /Users/rabble/code/divine/divine-sky/scripts/smoke-divine-atproto-login.sh
```

Expected: PASS for `rabble.divine.video`.

- [ ] **Step 3: Verify both login modes**

Run:

- one legacy session login against `pds.divine.video`
- one OAuth login from handle through `entryway.divine.video`

Expected: both succeed for the canary user.

- [ ] **Step 4: Expand the migration to all active `*.divine.video` users**

Do not expand until the canary is stable.

- [ ] **Step 5: Re-run the smoke script for a migrated existing user and a newly provisioned user**

Expected: PASS for both.

- [ ] **Step 6: Commit final runbook updates**

```bash
git -C /Users/rabble/code/divine/divine-sky add docs/runbooks
git -C /Users/rabble/code/divine/divine-sky commit -m "docs: add divine atproto login rollout runbooks"
```
