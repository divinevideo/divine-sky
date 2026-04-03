# entryway.divine.video Phase 2 Normalization Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Normalize Phase 2 so users can sign in to Bluesky-compatible ATProto clients with `username.divine.video` through `pds.divine.video` plus `entryway.divine.video`, while `login.divine.video` stays the Keycast human console for DiVine + Nostr auth and lifecycle control.

**Architecture:** Treat `entryway.divine.video` as the only public ATProto Authorization Server origin. Keep Keycast as the source of truth for account sessions, consent, and ATProto lifecycle state, but make the public protocol contract host-aware: `entryway` serves ATProto authorization-server behavior, `pds` serves protected-resource behavior, and `login` remains a human-facing control plane. The implementation work is mostly contract tightening and normalization across tests, config, smoke scripts, and docs rather than inventing a new OAuth flow from scratch.

**Tech Stack:** Rust, Axum, `keycast`, `rsky-pds`, SQLx/Postgres, ATProto OAuth metadata, PAR, DPoP, markdown runbooks, shell smoke scripts.

---

## Chunk 1: Normalize The Written Contract

### Task 1: Replace or supersede the old `login.divine.video` auth-server story

**Files:**
- Create: `docs/superpowers/specs/2026-04-03-entryway-divine-video-atproto-login-boundary-design.md`
- Modify: `docs/superpowers/specs/2026-03-28-login-divine-video-atproto-auth-server-design.md`
- Modify: `docs/superpowers/plans/2026-03-28-login-divine-video-atproto-auth-server.md`
- Reference: `docs/runbooks/login-divine-video.md`

- [ ] **Step 1: Add the normalized design doc**

Write a spec that states:
- `username.divine.video` is the public handle
- `pds.divine.video` is the protected resource
- `entryway.divine.video` is the ATProto Authorization Server
- `login.divine.video` is the Keycast human console

- [ ] **Step 2: Mark the older `login`-as-auth-server spec as superseded**

Add a short note near the top of `2026-03-28-login-divine-video-atproto-auth-server-design.md` pointing readers to the newer `entryway` boundary design.

- [ ] **Step 3: Mark the older `login`-as-auth-server plan as superseded**

Add a short note near the top of `2026-03-28-login-divine-video-atproto-auth-server.md` that Phase 2 should now be executed against the `entryway`-normalized contract.

- [ ] **Step 4: Verify the docs cross-reference cleanly**

Run:

```bash
cd /Users/rabble/code/divine/divine-sky
rg -n "login\\.divine\\.video.*Authorization Server|entryway\\.divine\\.video" docs/superpowers/specs docs/superpowers/plans docs/runbooks
```

Expected: older `login` references remain only where intentionally marked historical or superseded.

### Task 2: Reconcile runbooks and smoke docs around `entryway.divine.video`

**Files:**
- Modify: `docs/runbooks/login-divine-video.md`
- Modify: `docs/runbooks/atproto-auth-server-smoke-test.md`
- Modify: `docs/runbooks/launch-checklist.md`
- Modify: `scripts/smoke-divine-atproto-login.sh`

- [ ] **Step 1: Update the smoke test hostname expectations**

Ensure the runbook and smoke script both expect:
- `https://pds.divine.video/.well-known/oauth-protected-resource`
- `authorization_servers = ["https://entryway.divine.video"]`
- `https://entryway.divine.video/.well-known/oauth-authorization-server`

- [ ] **Step 2: Make the login runbook explicit about Keycast’s role**

Add a short contract summary that says Keycast owns:
- user session
- consent
- lifecycle state
- enable / disable UX

But not the public ATProto Authorization Server hostname.

- [ ] **Step 3: Update the launch checklist**

Include a deploy-time check that `entryway.divine.video` is the issuer in live auth-server metadata and that `login.divine.video` is not referenced as the ATProto Authorization Server in current customer-facing docs.

- [ ] **Step 4: Validate the docs and script surface**

Run:

```bash
cd /Users/rabble/code/divine/divine-sky
bash scripts/smoke-divine-atproto-login.sh --help
ruby -e 'require "yaml"; YAML.load_file("docs/runbooks/launch-checklist.md") rescue nil'
```

Expected: the smoke script still parses and the runbook edits are syntactically clean.

## Chunk 2: Tighten The Keycast Host Boundary

### Task 3: Add failing tests that prove `entryway` is the auth-server host and `login` is not

**Files:**
- Modify: `/Users/rabble/code/divine/keycast/api/tests/atproto_oauth_metadata_test.rs`
- Modify: `/Users/rabble/code/divine/keycast/api/tests/atproto_par_test.rs`
- Modify: `/Users/rabble/code/divine/keycast/api/tests/atproto_dpop_token_test.rs`
- Reference: `/Users/rabble/code/divine/keycast/api/src/api/http/atproto_oauth.rs`

- [ ] **Step 1: Add a failing metadata test for the login host**

Assert that requests with:

```text
Host: login.divine.video
```

do not advertise `login.divine.video` as the issuer when the public contract is `entryway.divine.video`.

- [ ] **Step 2: Add a failing metadata test for forwarded-host handling**

Assert that when the incoming deployment terminates on the same service but forwards:

```text
Host: login.divine.video
X-Forwarded-Host: entryway.divine.video
```

the metadata still resolves to `https://entryway.divine.video`.

- [ ] **Step 3: Add a failing PAR / token test for wrong-host rejection**

Assert that DPoP `htu` and issuer-sensitive request handling reject `login.divine.video` when the public ATProto origin should be `entryway.divine.video`.

- [ ] **Step 4: Run the focused keycast tests to verify they fail**

Run:

```bash
cd /Users/rabble/code/divine/keycast
cargo test -p keycast_api --test atproto_oauth_metadata_test --test atproto_par_test --test atproto_dpop_token_test -- --nocapture
```

Expected: FAIL if any host-boundary assumptions still leak `login.divine.video`.

### Task 4: Implement or tighten host-aware auth-server behavior in Keycast

**Files:**
- Modify: `/Users/rabble/code/divine/keycast/api/src/api/http/atproto_oauth.rs`
- Modify: `/Users/rabble/code/divine/keycast/api/src/api/http/atproto_oauth_metadata.rs`
- Modify: `/Users/rabble/code/divine/keycast/api/openapi.yaml`
- Modify: `/Users/rabble/code/divine/keycast/docs/DEPLOYMENT.md`

- [ ] **Step 1: Centralize the entryway-origin resolution**

Make one helper authoritative for:
- issuer
- authorization endpoint
- token endpoint
- PAR endpoint
- host matching for forwarded traffic

- [ ] **Step 2: Fail closed on the wrong public host**

If a request arrives on `login.divine.video` for public ATProto auth-server metadata, either:
- return `404`, or
- redirect only if the existing ATProto client behavior is proven safe

Prefer `404` unless a failing test proves the redirect is required and safe.

- [ ] **Step 3: Update docs and API examples**

OpenAPI and deployment docs should reference `entryway.divine.video` for ATProto auth-server examples, while leaving `login.divine.video` examples intact for DiVine and Nostr flows.

- [ ] **Step 4: Re-run the focused keycast tests**

Run:

```bash
cd /Users/rabble/code/divine/keycast
cargo test -p keycast_api --test atproto_oauth_metadata_test --test atproto_par_test --test atproto_dpop_token_test -- --nocapture
cargo clippy -p keycast_api --all-targets -- -D warnings
```

Expected: PASS.

## Chunk 3: Tighten The PDS Discovery Contract

### Task 5: Add failing tests that `pds.divine.video` only advertises `entryway.divine.video`

**Files:**
- Modify: `/Users/rabble/code/divine/rsky/rsky-pds/tests/integration_tests.rs`
- Reference: `/Users/rabble/code/divine/rsky/rsky-pds/src/well_known.rs`
- Reference: `/Users/rabble/code/divine/rsky/rsky-pds/src/config/mod.rs`

- [ ] **Step 1: Add a failing protected-resource metadata assertion**

Assert:

```json
{
  "authorization_servers": ["https://entryway.divine.video"]
}
```

and reject responses that still name `login.divine.video`.

- [ ] **Step 2: Add a failing env-driven config test**

Assert that the protected-resource metadata uses the configured entryway URL rather than a hard-coded login URL.

- [ ] **Step 3: Run the focused PDS integration test**

Run:

```bash
cd /Users/rabble/code/divine/rsky
cargo test -p rsky-pds --test integration_tests -- --nocapture
```

Expected: FAIL if discovery still leaks the wrong hostname.

### Task 6: Implement or tighten PDS protected-resource metadata and config

**Files:**
- Modify: `/Users/rabble/code/divine/rsky/rsky-pds/src/well_known.rs`
- Modify: `/Users/rabble/code/divine/rsky/rsky-pds/src/config/mod.rs`
- Modify: `/Users/rabble/code/divine/rsky/rsky-pds/src/auth_verifier.rs`

- [ ] **Step 1: Centralize entryway-origin config**

Use a single config path for:
- protected-resource metadata `authorization_servers`
- external token issuer validation
- any ATProto auth-server docs or comments

- [ ] **Step 2: Remove stale `login` phrasing from the code comments**

Update comments that still describe the flow as "`login.divine.video / keycast`" if the public ATProto contract is now `entryway.divine.video`.

- [ ] **Step 3: Re-run the focused PDS tests**

Run:

```bash
cd /Users/rabble/code/divine/rsky
cargo test -p rsky-pds --test integration_tests -- --nocapture
```

Expected: PASS.

## Chunk 4: End-To-End Interop Verification

### Task 7: Add a production-style interop canary for the public chain

**Files:**
- Modify: `scripts/smoke-divine-atproto-login.sh`
- Modify: `docs/runbooks/divine-atproto-login-canary.md`
- Modify: `docs/runbooks/atproto-auth-server-smoke-test.md`

- [ ] **Step 1: Add a public-client canary path**

Cover:
- handle typed as `username.divine.video`
- PDS protected-resource discovery
- auth-server discovery on `entryway.divine.video`
- PAR
- browser authorization
- token exchange
- authenticated PDS request

- [ ] **Step 2: Add explicit host assertions**

The canary should fail if:
- `login.divine.video` is discovered as the ATProto Authorization Server
- `entryway.divine.video` serves HTML instead of JSON metadata
- `pds.divine.video` advertises anything other than `entryway.divine.video`

- [ ] **Step 3: Run the canary in dry-run or documented local mode**

Run:

```bash
cd /Users/rabble/code/divine/divine-sky
bash scripts/smoke-divine-atproto-login.sh
```

Expected: PASS in an environment with live entryway and PDS configuration.

### Task 8: Final verification and rollout signoff

**Files:**
- No code changes expected

- [ ] **Step 1: Re-run all focused contract checks**

Run:

```bash
cd /Users/rabble/code/divine/keycast
cargo test -p keycast_api --test atproto_oauth_metadata_test --test atproto_par_test --test atproto_dpop_token_test -- --nocapture

cd /Users/rabble/code/divine/rsky
cargo test -p rsky-pds --test integration_tests -- --nocapture

cd /Users/rabble/code/divine/divine-sky
bash scripts/smoke-divine-atproto-login.sh
```

- [ ] **Step 2: Verify the human/protocol split manually**

Confirm:
- `login.divine.video` is still the DiVine + Nostr Keycast UI
- `entryway.divine.video` is the ATProto Authorization Server
- `pds.divine.video` is the protected resource
- `username.divine.video` is the handle users enter in clients

- [ ] **Step 3: Commit the normalization work**

```bash
git -C /Users/rabble/code/divine/divine-sky add docs/superpowers/specs/2026-04-03-entryway-divine-video-atproto-login-boundary-design.md docs/superpowers/specs/2026-03-28-login-divine-video-atproto-auth-server-design.md docs/superpowers/plans/2026-04-03-entryway-divine-video-phase2-normalization.md docs/superpowers/plans/2026-03-28-login-divine-video-atproto-auth-server.md docs/runbooks/login-divine-video.md docs/runbooks/atproto-auth-server-smoke-test.md docs/runbooks/launch-checklist.md docs/runbooks/divine-atproto-login-canary.md scripts/smoke-divine-atproto-login.sh
git -C /Users/rabble/code/divine/keycast add api/src/api/http/atproto_oauth.rs api/src/api/http/atproto_oauth_metadata.rs api/openapi.yaml docs/DEPLOYMENT.md api/tests/atproto_oauth_metadata_test.rs api/tests/atproto_par_test.rs api/tests/atproto_dpop_token_test.rs
git -C /Users/rabble/code/divine/rsky add rsky-pds/src/well_known.rs rsky-pds/src/config/mod.rs rsky-pds/src/auth_verifier.rs rsky-pds/tests/integration_tests.rs
git -C /Users/rabble/code/divine/divine-sky commit -m "docs: normalize phase 2 around entryway auth"
git -C /Users/rabble/code/divine/keycast commit -m "fix: make entryway the public atproto auth host"
git -C /Users/rabble/code/divine/rsky commit -m "fix: advertise entryway as pds auth server"
```
