# Phase 2 ATProto Refresh + DPoP Completion Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the missing Phase 2 protocol work so the delegated auth server supports refresh-token rotation and end-to-end DPoP binding across token and protected-resource requests.

**Architecture:** Extend the dedicated ATProto OAuth session model in keycast instead of reusing generic OAuth tables, then enforce the same DPoP session key at PAR, the token endpoint, and `rsky-pds` protected resources. Keep access tokens short-lived JWTs signed by keycast, keep refresh tokens opaque and rotated on every use, bind both to a persisted DPoP JWK thumbprint (`jkt`), and issue server-provided `DPoP-Nonce` headers so the flow matches the atproto OAuth profile.

**Tech Stack:** Rust, Axum, Rocket, SQLx/Postgres, `jwt-simple`, `secp256k1`, ATProto OAuth profile, RFC 9449 DPoP.

---

## Chunk 1: Keycast token endpoint refresh grant and DPoP binding

### Task 1: Add failing tests for PAR DPoP initiation, auth-code DPoP binding, and refresh rotation

**Files:**
- Modify: `api/tests/atproto_oauth_http_test.rs`
- Modify: `api/tests/atproto_oauth_session_test.rs`
- Reference: `api/src/api/http/atproto_oauth.rs`
- Reference: `core/src/repositories/atproto_oauth_session.rs`

- [ ] Add a focused HTTP test that sends a valid DPoP proof on the PAR request and asserts:
  - the PAR response succeeds
  - the response includes a `DPoP-Nonce` header
  - the session row stores the initial session `dpop_jkt`
  - a missing or invalid PAR DPoP proof is rejected
- [ ] Add a focused HTTP test that sends a valid DPoP proof on the initial `authorization_code` token exchange and asserts:
  - the response still returns `token_type = "DPoP"`
  - the token endpoint requires the same session key established at PAR
  - the response includes a `DPoP-Nonce` header
  - a missing or wrong-key DPoP header is rejected
- [ ] Add a focused HTTP test for `grant_type=refresh_token` that:
  - exchanges a valid refresh token with the same DPoP key
  - receives a new access token and rotated refresh token
  - cannot reuse the old refresh token
- [ ] Add a focused HTTP test that refreshing with a different DPoP key fails.
- [ ] Run the focused tests and confirm they fail for the expected missing-behavior reasons.

Run: `DATABASE_URL=postgres://divine:divine_dev@localhost:5432/keycast_test cargo test --test atproto_oauth_http_test --test atproto_oauth_session_test -- --nocapture`

### Task 2: Implement PAR + token-endpoint DPoP validation, nonce handling, and refresh rotation

**Files:**
- Modify: `api/src/api/http/atproto_oauth.rs`
- Modify: `core/src/repositories/atproto_oauth_session.rs`
- Modify: `core/src/repositories/mod.rs` if helper exports change
- Modify: `api/Cargo.toml` only if a new JWT/JWK helper dependency is truly required

- [ ] Add token-request parsing for `refresh_token` and the `DPoP` request header.
- [ ] Add PAR request parsing for the `DPoP` request header and reject PAR requests that do not initiate DPoP.
- [ ] Implement DPoP proof validation for token-endpoint requests:
  - parse JWT header and payload
  - extract the public JWK from the proof header
  - verify the signature
  - verify `htm`, `htu`, `iat`, and `jti`
  - derive and return the JWK thumbprint (`jkt`)
- [ ] Implement server-provided nonce issuance and validation for PAR and token-endpoint DPoP requests, including `DPoP-Nonce` response headers and rejection of stale/missing nonce usage according to the atproto profile.
- [ ] Persist the PAR-established `jkt` on the ATProto OAuth session and require the same key throughout the session.
- [ ] Ensure issued access tokens carry the key-binding information needed by `rsky-pds` to verify the DPoP proof locally.
- [ ] Add repository support to look up the session by refresh-token hash and rotate token artifacts atomically when the refresh token is valid, unexpired, and not revoked.
- [ ] Enforce that refresh uses the same persisted `jkt`; reject mismatches.
- [ ] Keep access-token lifetime unchanged unless a failing test or spec check requires adjustment.
- [ ] Re-run the focused keycast tests until green.

Run: `DATABASE_URL=postgres://divine:divine_dev@localhost:5432/keycast_test cargo test --test atproto_oauth_http_test --test atproto_oauth_session_test -- --nocapture`

## Chunk 2: `rsky-pds` protected-resource DPoP enforcement

### Task 3: Add failing tests for DPoP-protected resource access

**Files:**
- Modify: `rsky-pds/tests/integration_tests.rs`
- Reference: `rsky-pds/src/auth_verifier.rs`

- [ ] Add a test that presents a keycast-issued access token with `Authorization: DPoP <token>` plus a matching `DPoP` proof and confirms the request succeeds.
- [ ] Add a test that presents the same token without a DPoP proof and confirm it is rejected.
- [ ] Add a test that presents a DPoP proof with the wrong `ath`, `htm`, `htu`, or key binding and confirm it is rejected.
- [ ] Add a test that verifies the protected-resource response includes or updates `DPoP-Nonce` as required by the atproto profile.
- [ ] Run the focused `rsky-pds` integration tests and confirm they fail for the expected missing DPoP checks.

Run: `cargo test -p rsky-pds --test integration_tests -- --nocapture`

### Task 4: Implement DPoP verification for protected resources

**Files:**
- Modify: `rsky-pds/src/auth_verifier.rs`
- Modify: `rsky-pds/Cargo.toml` only if a new JWT/JWK helper dependency is truly required
- Optionally create: `rsky-pds/src/auth_dpop.rs` if the parsing/validation logic becomes too large for `auth_verifier.rs`

- [ ] Require `Authorization: DPoP <access-token>` for externally issued ATProto access tokens that are marked as DPoP-bound.
- [ ] Parse and verify the `DPoP` proof JWT:
  - signature against the embedded public JWK
  - `htm` and `htu` against the current request
  - `ath` against the presented access token
  - `iat` freshness and unique `jti` within the replay window
- [ ] Read the access token’s binding information and enforce that the proof key matches the access-token key binding.
- [ ] Issue and validate protected-resource `DPoP-Nonce` headers so retried requests can complete the atproto profile flow.
- [ ] Preserve the existing bearer-token validation path for non-DPoP internal tokens unless a failing test proves it must change.
- [ ] Re-run the focused `rsky-pds` integration tests until green.

Run: `cargo test -p rsky-pds --test integration_tests -- --nocapture`

## Chunk 3: Contract updates and final verification

### Task 5: Update contract and runbook coverage

**Files:**
- Modify: `api/openapi.yaml`
- Modify: `docs/runbooks/login-divine-video.md`
- Modify: `docs/runbooks/launch-checklist.md`
- Modify: `docs/runbooks/atproto-auth-server-smoke-test.md`

- [ ] Update API and runbook material so it no longer claims refresh/DPoP are missing.
- [ ] Document the request shape for token refresh and protected-resource access with DPoP.
- [ ] Document PAR initiation and `DPoP-Nonce` behavior for the delegated flow.
- [ ] Call out any remaining intentional limits, such as replay-cache scope, if they are not fully implemented in this slice.

### Task 6: Final verification

**Files:**
- No code changes expected

- [ ] Run the full keycast Phase 2 ATProto verification command.
- [ ] Run the full `rsky-pds` integration command with the required `libpq` environment.
- [ ] Re-read the Phase 2 spec acceptance criteria and confirm the new implementation covers the refresh-token revocation requirement in addition to discovery, auth code, and PDS access.

Run: `DATABASE_URL=postgres://divine:divine_dev@localhost:5432/keycast_test cargo test --test oauth_unit_test --test atproto_oauth_metadata_test --test atproto_oauth_session_test --test atproto_oauth_http_test -- --nocapture`

Run: `cargo test -p rsky-pds --test integration_tests -- --nocapture`
