# login.divine.video ATProto Boundary Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `login.divine.video` behave honestly for ATProto clients by failing closed on protocol paths it does not own, and add root-level ATProto authorization-server metadata only when Divine is ready to use `login.divine.video` as a real ATProto Authorization Server, with the concrete end-state that `rabble.divine.video` can eventually sign in through the official Bluesky app without the login host misreporting its protocol role.

**Architecture:** Keep `login.divine.video` aligned with the current runbook boundary: it remains the authenticated control plane for account linking and may become an ATProto Authorization Server, but it is not the handle-resolution host and not the PDS read surface. The implementation splits into three layers: protocol-surface tests that prove the current failure, a router/fallback hardening change in `keycast` so the SPA never impersonates ATProto endpoints, and an optional metadata layer gated behind explicit config so the host only advertises ATProto auth capabilities when the downstream PDS/entryway path is actually ready.

**Tech Stack:** Rust, Axum, tower-http, SvelteKit static build, Cloud Run, AT Protocol OAuth profile, RFC 8414, OAuth Protected Resource Metadata

---

## Assumptions And Boundaries

- `login.divine.video` must not serve `/.well-known/atproto-did`; that stays on `divine-router` per [docs/runbooks/login-divine-video.md](/Users/rabble/code/divine/divine-sky/docs/runbooks/login-divine-video.md).
- `login.divine.video` should not serve `/.well-known/oauth-protected-resource`, `com.atproto.server.describeServer`, or `com.atproto.identity.resolveHandle` unless it later becomes the actual Resource Server or a deliberate protocol proxy. Today it should return `404`, not HTML.
- Public ATProto sign-in for `username.divine.video` still depends on the real Divine PDS or entryway exposing the correct Resource Server metadata and token semantics. This plan only updates `login.divine.video`.
- The acceptance target for this workstream is: the official Bluesky app can start the sign-in flow for `rabble.divine.video` without being sent misleading HTML from `login.divine.video` for ATProto discovery endpoints. Completing the full login still requires the downstream PDS or entryway work listed below.
- Implementation work in this plan is primarily in `/Users/rabble/code/divine/keycast`, with one runbook update in `/Users/rabble/code/divine/divine-sky`.

## File Structure

- Modify: `/Users/rabble/code/divine/keycast/keycast/src/main.rs`
  Route composition, SPA fallback behavior, and root-level well-known wiring.
- Create: `/Users/rabble/code/divine/keycast/keycast/src/http_surface.rs`
  Focused helper for injected `index.html` responses, reserved protocol path classification, and non-HTML fallback responses.
- Create: `/Users/rabble/code/divine/keycast/keycast/tests/atproto_surface_test.rs`
  Integration tests that exercise the real root router behavior for `/`, `/.well-known/*`, and `/xrpc/*`.
- Create: `/Users/rabble/code/divine/keycast/api/src/api/http/atproto_auth_server.rs`
  Optional ATProto Authorization Server metadata handler and config parsing for the root metadata document.
- Modify: `/Users/rabble/code/divine/keycast/api/src/api/http/mod.rs`
  Export the new ATProto auth-server handler module.
- Create: `/Users/rabble/code/divine/keycast/api/tests/atproto_auth_metadata_test.rs`
  Tests for disabled and enabled metadata behavior.
- Create: `/Users/rabble/code/divine/keycast/scripts/smoke-login-divine-atproto.sh`
  Post-deploy curl checks for protocol boundary behavior.
- Modify: `/Users/rabble/code/divine/keycast/docs/DEPLOYMENT.md`
  Deployment and smoke-check instructions for the new protocol behavior.
- Modify: `/Users/rabble/code/divine/divine-sky/docs/runbooks/login-divine-video.md`
  Clarify which ATProto endpoints `login.divine.video` owns and which hosts must serve the remaining ATProto surface.

## Chunk 1: Stop The SPA From Impersonating ATProto Endpoints

### Task 1: Add the failing protocol-surface tests in `keycast`

**Files:**
- Create: `/Users/rabble/code/divine/keycast/keycast/tests/atproto_surface_test.rs`

- [ ] **Step 1: Write the failing root-vs-protocol behavior tests**

Add tests that boot the `keycast` HTTP app and assert:
- `GET /` returns `200` with `text/html`
- `GET /.well-known/nostr.json` still returns `200`
- `GET /.well-known/oauth-authorization-server` returns `404` when ATProto auth-server mode is disabled
- `GET /.well-known/oauth-protected-resource` returns `404`
- `GET /.well-known/atproto-did` returns `404`
- `GET /xrpc/com.atproto.server.describeServer` returns `404`
- `GET /xrpc/com.atproto.identity.resolveHandle?handle=alice.divine.video` returns `404`

- [ ] **Step 2: Run the focused surface test to verify it fails**

Run:

```bash
cargo test -p keycast --test atproto_surface_test -- --nocapture
```

Expected: FAIL because the current `fallback_service` serves `index.html` for unknown `/.well-known/*` and `/xrpc/*` paths.

### Task 2: Harden the root router so reserved protocol paths fail closed

**Files:**
- Create: `/Users/rabble/code/divine/keycast/keycast/src/http_surface.rs`
- Modify: `/Users/rabble/code/divine/keycast/keycast/src/main.rs`

- [ ] **Step 1: Extract the current HTML injection logic into `http_surface.rs`**

Move the `index.html` injection helper and SPA fallback logic out of `main.rs` so the route classification logic is testable on its own.

- [ ] **Step 2: Add a reserved-path classifier**

Implement a small helper such as:

```rust
pub fn is_reserved_protocol_path(path: &str) -> bool {
    path.starts_with("/xrpc/")
        || path == "/xrpc"
        || path == "/.well-known/oauth-authorization-server"
        || path == "/.well-known/oauth-protected-resource"
        || path == "/.well-known/atproto-did"
}
```

The helper should be intentionally narrow: it must protect the ATProto paths above without breaking `/.well-known/nostr.json`, Apple app association files, or real static assets.

- [ ] **Step 3: Change the SPA fallback to return `404` for reserved protocol paths**

When the request path is reserved and there is no explicit route match, return:

```rust
(StatusCode::NOT_FOUND, "Not found")
```

Do not return `index.html`, do not redirect, and do not emit HTML for these paths.

- [ ] **Step 4: Run the focused surface test to verify it now passes**

Run:

```bash
cargo test -p keycast --test atproto_surface_test -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Run a focused compile check for the binary crate**

Run:

```bash
cargo check -p keycast --tests
```

Expected: PASS.

- [ ] **Step 6: Commit the boundary fix**

Run:

```bash
git -C /Users/rabble/code/divine/keycast add keycast/src/main.rs keycast/src/http_surface.rs keycast/tests/atproto_surface_test.rs
git -C /Users/rabble/code/divine/keycast commit -m "fix: fail closed on atproto protocol paths"
```

## Chunk 2: Add ATProto Auth-Server Metadata Only Behind An Explicit Gate

### Task 3: Add failing tests for the metadata document

**Files:**
- Create: `/Users/rabble/code/divine/keycast/api/tests/atproto_auth_metadata_test.rs`

- [ ] **Step 1: Write the disabled-mode metadata test**

Add a test that starts the route with `ATPROTO_AUTH_SERVER_ENABLED` unset and asserts:
- `GET /.well-known/oauth-authorization-server` returns `404`
- content type is not `text/html`

- [ ] **Step 2: Write the enabled-mode metadata test**

Add a test that starts the route with `ATPROTO_AUTH_SERVER_ENABLED=true` and `APP_URL=https://login.divine.video` and asserts a `200 application/json` response containing at least:
- `"issuer": "https://login.divine.video"`
- `"authorization_endpoint": "https://login.divine.video/api/oauth/authorize"`
- `"token_endpoint": "https://login.divine.video/api/oauth/token"`
- `"pushed_authorization_request_endpoint": "https://login.divine.video/api/oauth/par"`
- `"scopes_supported"` including `"atproto"`
- `"authorization_response_iss_parameter_supported": true`
- `"require_pushed_authorization_requests": true`

- [ ] **Step 3: Run the metadata tests to verify they fail**

Run:

```bash
cargo test -p keycast-api atproto_auth_metadata -- --nocapture
```

Expected: FAIL because the handler and config do not exist yet.

### Task 4: Implement the optional metadata handler

**Files:**
- Create: `/Users/rabble/code/divine/keycast/api/src/api/http/atproto_auth_server.rs`
- Modify: `/Users/rabble/code/divine/keycast/api/src/api/http/mod.rs`
- Modify: `/Users/rabble/code/divine/keycast/keycast/src/main.rs`

- [ ] **Step 1: Add a small config reader for auth-server mode**

Implement config helpers that read:
- `ATPROTO_AUTH_SERVER_ENABLED`
- `APP_URL` or `BASE_URL`

If the feature is disabled or the issuer URL is missing or malformed, the handler should decline to advertise auth-server metadata and let the route return `404`.

- [ ] **Step 2: Implement the metadata document handler**

Return a JSON object with the minimum ATProto OAuth fields required by the spec, including:
- `issuer`
- `authorization_endpoint`
- `token_endpoint`
- `pushed_authorization_request_endpoint`
- `response_types_supported`
- `grant_types_supported`
- `code_challenge_methods_supported`
- `scopes_supported`
- `authorization_response_iss_parameter_supported`
- `require_pushed_authorization_requests`

Keep this handler free of token logic. It is only allowed to advertise metadata when the downstream auth-server behavior is genuinely available.

- [ ] **Step 3: Mount the well-known route at the root router**

Mount:

```rust
/.well-known/oauth-authorization-server
```

at the root level in `keycast/src/main.rs`, not under `/api`, so it matches ATProto discovery rules.

- [ ] **Step 4: Re-run the metadata tests**

Run:

```bash
cargo test -p keycast-api atproto_auth_metadata -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Gate rollout on the real auth-server contract**

Before enabling `ATPROTO_AUTH_SERVER_ENABLED` in production, verify that Divine-hosted PDS or entryway infrastructure actually points clients at `https://login.divine.video` as the Authorization Server. If not, keep the feature flag off and stop after Chunk 1.

- [ ] **Step 6: Commit the metadata work**

Run:

```bash
git -C /Users/rabble/code/divine/keycast add api/src/api/http/mod.rs api/src/api/http/atproto_auth_server.rs api/tests/atproto_auth_metadata_test.rs keycast/src/main.rs
git -C /Users/rabble/code/divine/keycast commit -m "feat: add optional atproto auth server metadata"
```

## Chunk 3: Add Deployment Smoke Checks And Boundary Documentation

### Task 5: Add post-deploy smoke checks for `login.divine.video`

**Files:**
- Create: `/Users/rabble/code/divine/keycast/scripts/smoke-login-divine-atproto.sh`
- Modify: `/Users/rabble/code/divine/keycast/docs/DEPLOYMENT.md`

- [ ] **Step 1: Add the smoke script**

Create a script that runs:

```bash
curl -fsS -o /dev/null -w "%{http_code}\n" https://login.divine.video/
curl -fsS -o /dev/null -w "%{http_code}\n" https://login.divine.video/.well-known/nostr.json
curl -sS -o /tmp/login-authz.json -D /tmp/login-authz.headers https://login.divine.video/.well-known/oauth-authorization-server
curl -sS -o /tmp/login-protected.body -D /tmp/login-protected.headers https://login.divine.video/.well-known/oauth-protected-resource
curl -sS -o /tmp/login-describe.body -D /tmp/login-describe.headers https://login.divine.video/xrpc/com.atproto.server.describeServer
```

The script should fail if:
- `/` is not `200`
- `/.well-known/nostr.json` is not `200`
- `/.well-known/oauth-protected-resource` is anything other than `404`
- `/xrpc/com.atproto.server.describeServer` is anything other than `404`
- `/.well-known/oauth-authorization-server` is HTML, regardless of whether it is `404` or `200`

- [ ] **Step 2: Run the smoke script against production manually**

Run:

```bash
bash /Users/rabble/code/divine/keycast/scripts/smoke-login-divine-atproto.sh
```

Expected:
- PASS after Chunk 1 if auth-server metadata is disabled
- PASS after Chunk 2 with either `404` or `200 application/json` on the auth-server metadata endpoint, depending on the feature flag

### Task 6: Update the deployment and runbook docs

**Files:**
- Modify: `/Users/rabble/code/divine/keycast/docs/DEPLOYMENT.md`
- Modify: `/Users/rabble/code/divine/divine-sky/docs/runbooks/login-divine-video.md`

- [ ] **Step 1: Document the reserved-path behavior in `keycast` deployment docs**

Add a section that states:
- unknown `/.well-known/*` and `/xrpc/*` ATProto paths must never render the SPA
- `/.well-known/oauth-authorization-server` is disabled unless explicitly enabled
- `/.well-known/oauth-protected-resource`, `/.well-known/atproto-did`, and `/xrpc/*` are not `login.divine.video` responsibilities

- [ ] **Step 2: Update the Divine runbook boundary**

Extend `docs/runbooks/login-divine-video.md` with an operator note that:
- `login.divine.video` may optionally advertise ATProto Authorization Server metadata
- the real Divine PDS or entryway must publish `/.well-known/oauth-protected-resource`
- `divine-router` remains the only host for `/.well-known/atproto-did`
- ATProto client failures against `login.divine.video/xrpc/*` are expected and correct unless a future service intentionally owns those routes

- [ ] **Step 3: Run the focused verification set**

Run:

```bash
cargo test -p keycast --test atproto_surface_test -- --nocapture
cargo test -p keycast-api atproto_auth_metadata -- --nocapture
cargo check -p keycast --tests
bash /Users/rabble/code/divine/keycast/scripts/smoke-login-divine-atproto.sh
```

Expected: PASS.

## Dependencies Outside This Plan

The following are required before public ATProto login for `username.divine.video` will work end to end, but they are not implemented by this plan:

- The Divine PDS or entryway must publish `/.well-known/oauth-protected-resource` with `authorization_servers` pointing to the real Authorization Server.
- The Divine PDS must accept the access tokens minted by that Authorization Server and expose the expected ATProto XRPC surface.
- Handle resolution for `username.divine.video` must continue to resolve via `_atproto.<handle>` or `/.well-known/atproto-did` on the router-hosting side, not on `login.divine.video`.
- `rabble.divine.video` must resolve to a DID whose service document points the Bluesky client at the correct Divine PDS or entryway, not at `login.divine.video` as though it were the PDS itself.
- If Divine decides not to use `login.divine.video` as the ATProto Authorization Server, leave `ATPROTO_AUTH_SERVER_ENABLED` disabled and write a separate plan in the owning PDS or entryway repo instead.
