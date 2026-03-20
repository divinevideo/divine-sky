# Launch Checklist

## Preflight

- Confirm `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `bash scripts/test-workspace.sh` pass on the release candidate.
- Verify `login.divine.video` can opt in, disable, export, and serve `/.well-known/atproto-did` for a ready account.
- Confirm `pds.divine.video` healthchecks, MinIO buckets, and relay connectivity are green before enabling traffic.

## Rollout Controls

- Enable BGS crawl only after relay replay offsets and PDS write auth are verified in staging.
- Review rate limits for relay intake, Blossom fetches, and PDS XRPC writes before widening the cohort.
- Start with an internal cohort, then a small creator cohort, then broader opt-in traffic.

## Safety

- Ensure alerting exists for relay disconnect loops, PDS write failures, and asset-manifest persistence failures.
- Keep a rollback path that disables new opt-ins and stops the bridge without deleting existing AT records.
- Route DMCA and takedown intake into the moderation queue before enabling public creator onboarding.

## Ops

- Record the active `RELAY_SOURCE_NAME`, PDS auth source, and deployed compose/image versions for each rollout.
- Confirm support staff have the disable/export runbook and a tested contact path for account recovery issues.
