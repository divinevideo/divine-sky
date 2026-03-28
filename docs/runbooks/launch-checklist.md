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
- Verify a verified cookie-auth user can open `settings/security` and see the `Bluesky Account` card without unlocking private-key export first.
- Verify keycast `/api/user/atproto/enable`, `/status`, and `/disable` work for an authenticated user.
- Verify the settings UI visibly covers the expected lifecycle states:
  - claim username
  - Bluesky disabled with enable CTA
  - pending
  - ready with DID shown
  - failed with retry path
  - disabled with re-enable path
- Verify `divine-handle-gateway` can POST lifecycle callbacks into keycast `/api/internal/atproto/state`.
- Verify `divine-name-server` receives ATProto readiness updates and publishes them to Fastly KV.
- Verify `divine-router` serves `/.well-known/atproto-did` only for active + ready usernames and returns `404` otherwise.
- Verify `divine-handle-gateway` does not present itself as a public `/.well-known/atproto-did` host.
- Confirm `pds.divine.video` healthchecks, MinIO buckets, and relay connectivity are green before enabling traffic.

## Rollout Controls

- Keep `FeatureFlag.atprotoPublishing` off in mobile by default until backend verification is complete.
- Keep `enableAtprotoPublishing` off in web by default until rollout is explicitly enabled.
- Do not widen rollout until the self-serve `settings/security` flow has passed the smoke test end to end.
- Enable BGS crawl only after relay replay offsets and PDS write auth are verified in staging.
- Review rate limits for relay intake, Blossom fetches, and PDS XRPC writes before widening the cohort.
- Start with an internal cohort, then a small creator cohort, then broader opt-in traffic.

## Safety

- Ensure alerting exists for relay disconnect loops, PDS write failures, and asset-manifest persistence failures.
- Keep a rollback path that disables new opt-ins and stops the bridge without deleting existing AT records.
- Confirm disable flow clears public `atproto_did` resolution and prevents new mirrored posts.
- Route DMCA and takedown intake into the moderation queue before enabling public creator onboarding.

## Rollback Gates

- Record the previously known-good `rsky-pds` image tag before deploying the DID-resolution patch.
- If the patched PDS breaks account creation, revert the staging overlay in `../divine-iac-coreconfig` to the previous `rsky-pds` image tag and resync ArgoCD.
- If the Fastly edge rollout is wrong, revert the `divine-router` service to the previous published package and purge the service again.
- If the user-facing ATProto path must be shut off, disable keycast opt-in by removing the staging runtime control-plane wiring or rolling back the keycast staging overlay without deleting existing AT repos.
- After any disable or rollback, confirm the Fastly KV record for the canary username no longer advertises `atproto_did` and `atproto_state = ready`.

## Ops

- Record the active `RELAY_SOURCE_NAME`, PDS auth source, and deployed compose/image versions for each rollout.
- Confirm support staff have the disable/export runbook and a tested contact path for account recovery issues.
- Confirm support staff understand the state model: claimed username does not imply ATProto ready.
- Confirm support staff know that `ready` in `login.divine.video` means both public DID resolution and future cross-post eligibility are active.
- Confirm the canonical architecture boundary is still intact:
  - keycast owns consent/lifecycle
  - divine-handle-gateway syncs ready/failed/disabled transitions back into keycast
  - divine-name-server owns public read model
  - divine-router remains read-only
  - divine-atbridge only publishes when `crosspost_enabled && ready`
