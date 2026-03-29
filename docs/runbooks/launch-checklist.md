# Launch Checklist

## Deploy Contract

- Confirm `divine-iac-coreconfig` is the source of truth for staging and production manifests, secrets, and routes.
- Confirm the `sky` namespace exists for `divine-sky` workloads.
- Confirm `divine-atbridge` and `divine-handle-gateway` are internal-only services.
- Confirm `divine-feedgen` and `divine-labeler` are the only public services.
- Confirm public hostnames are `feed.staging.dvines.org`, `feed.divine.video`, `labeler.staging.dvines.org`, and `labeler.divine.video`.

## Preflight

- Confirm `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `bash scripts/test-workspace.sh` pass on the release candidate.
- Verify keycast can claim usernames without enabling ATProto by default.
- Verify keycast `/api/user/atproto/enable`, `/status`, and `/disable` work for an authenticated user.
- Verify the PDS publishes `/.well-known/oauth-protected-resource` with `authorization_servers = ["https://login.divine.video"]`.
- Verify `login.divine.video` publishes `/.well-known/oauth-authorization-server` with PAR, token, issuer, `private_key_jwt`, and `client_id_metadata_document_supported = true` metadata for the ATProto auth-server surface.
- Verify the public key derived from keycast `ATPROTO_OAUTH_JWT_PRIVATE_KEY_HEX` matches `rsky-pds` `PDS_ENTRYWAY_JWT_PUBLIC_KEY_HEX`.
- Verify a `ready` linked account can complete PAR, browser approval, authorization-code exchange, refresh-token rotation, and `com.atproto.server.getSession` against the PDS with DPoP proofs and nonce challenges.
- Verify both public-client (`token_endpoint_auth_method = none`) and confidential-client (`private_key_jwt`) delegated login flows succeed with the same DPoP-bound session semantics.
- Verify `divine-handle-gateway` can POST lifecycle callbacks into keycast `/api/internal/atproto/state`.
- Verify `divine-name-server` receives ATProto readiness updates and publishes them to Fastly KV.
- Verify `divine-router` serves `/.well-known/atproto-did` only for active + ready usernames and returns `404` otherwise.
- Verify `divine-handle-gateway` does not present itself as a public `/.well-known/atproto-did` host.
- Confirm `pds.divine.video` healthchecks, MinIO buckets, and relay connectivity are green before enabling traffic.

## Rollout Controls

- Keep `FeatureFlag.atprotoPublishing` off in mobile by default until backend verification is complete.
- Keep `enableAtprotoPublishing` off in web by default until rollout is explicitly enabled.
- Keep delegated external-app login scoped to an internal or canary cohort until ATProto auth-server smoke tests are repeatable.
- Enable BGS crawl only after relay replay offsets and PDS write auth are verified in staging.
- Review rate limits for relay intake, Blossom fetches, and PDS XRPC writes before widening the cohort.
- Start with an internal cohort, then a small creator cohort, then broader opt-in traffic.

## Safety

- Ensure alerting exists for relay disconnect loops, PDS write failures, and asset-manifest persistence failures.
- Keep a rollback path that disables new opt-ins and stops the bridge without deleting existing AT records.
- Confirm disable flow clears public `atproto_did` resolution and prevents new mirrored posts.
- Confirm disabling the account blocks new delegated app approvals on `login.divine.video` and revokes active delegated refresh sessions so refresh fails immediately.
- Treat externally issued access tokens as short-lived DPoP-bound tokens during rollout; current revocation is immediate for new approvals and refreshes, but existing access tokens expire out naturally.
- Treat DPoP nonce and replay caches as per-instance state during rollout; keep canaries small and watch for multi-instance retry mismatches until cache distribution exists.
- Route DMCA and takedown intake into the moderation queue before enabling public creator onboarding.

## Rollback Gates

- Record the previously known-good `rsky-pds` image tag before deploying the DID-resolution patch.
- If the patched PDS breaks account creation, revert the staging overlay in `../divine-iac-coreconfig` to the previous `rsky-pds` image tag and resync ArgoCD.
- If the Fastly edge rollout is wrong, revert the `divine-router` service to the previous published package and purge the service again.
- If the user-facing ATProto path must be shut off, disable keycast opt-in by removing the staging runtime control-plane wiring or rolling back the keycast staging overlay without deleting existing AT repos.
- If delegated auth breaks, remove `PDS_ENTRYWAY_URL` / `PDS_ENTRYWAY_JWT_PUBLIC_KEY_HEX` from the PDS runtime and roll keycast back to the previous auth-server signing key or image before reopening traffic.
- After any disable or rollback, confirm the Fastly KV record for the canary username no longer advertises `atproto_did` and `atproto_state = ready`.

## Ops

- Record the active `RELAY_SOURCE_NAME`, PDS auth source, and deployed compose/image versions for each rollout.
- Record the delegated auth-server issuer, the deployed PDS entryway trust config, and the auth-server signing-key rollout version.
- Confirm support staff have the disable/export runbook and a tested contact path for account recovery issues.
- Confirm support staff understand the state model: claimed username does not imply ATProto ready.
- Confirm support staff understand the delegated auth caveat: a disabled account stops new approvals and refreshes immediately, but an already-issued access token can continue until expiry.
- Confirm the canonical architecture boundary is still intact:
  - keycast owns consent/lifecycle
  - keycast owns the delegated ATProto Authorization Server surface
  - divine-handle-gateway syncs ready/failed/disabled transitions back into keycast
  - divine-name-server owns public read model
  - divine-router remains read-only
  - rsky-pds remains the protected resource and validates external auth-server tokens
  - divine-atbridge only publishes when `crosspost_enabled && ready`
