# ATProto Opt-In Smoke Test

Use this checklist to validate the end-to-end ATProto opt-in flow across `rsky-pds`, `keycast`, `divine-sky`, `divine-name-server`, Fastly KV, and `divine-router`.

## Preflight

1. Run the PDS canary first:
   ```bash
   PDS_URL=https://pds.staging.dvines.org \
   PDS_ADMIN_PASSWORD=... \
   CANARY_HANDLE=atproto-canary-$(date +%s).staging.dvines.org \
   CANARY_DID=did:plc:... \
   bash scripts/staging-pds-did-smoke.sh
   ```
2. Verify `curl -fsS https://pds.staging.dvines.org/xrpc/_health` returns `200`.
3. Verify `curl -fsS https://login.staging.dvines.org/api/user/atproto/status` is reachable.

## Happy Path

1. Create or log in to a user in keycast.
2. Claim `username.divine.video`.
3. Verify `https://divine.video/.well-known/nostr.json?name=username` or the equivalent subdomain NIP-05 response.
4. Confirm ATProto is still disabled immediately after claim.
   - expected: `enabled = false`
   - expected: `state = null`
5. Enable ATProto from the authenticated client surface.
6. Verify keycast status reaches `pending`.
7. Verify the same user reaches `ready`.
   - keycast status endpoint shows `state = ready`
   - `divine-sky` `account_links` shows `pending -> ready`
8. Inspect the `divine-name-server` D1 row for the username.
   - expected: `atproto_did = did:plc:...`
   - expected: `atproto_state = ready`
9. Inspect the Fastly KV record for `user:<username>`.
   - expected payload fields:
     - `status = active`
     - `atproto_did = did:plc:...`
     - `atproto_state = ready`
10. Verify `divine-router` serves `https://username.divine.video/.well-known/atproto-did` and returns the bare DID.
11. Publish a Nostr video for the opted-in user.
12. Verify the mirrored ATProto post exists after the user is `ready`.
13. Disable ATProto.
14. Verify keycast status reaches `disabled`.
15. Verify future mirrored posts stop and `divine-router` returns `404` for `/.well-known/atproto-did`.

## Failure Checks

- Username claim must not auto-enable ATProto.
- `pending`, `failed`, and `disabled` users must not resolve `/.well-known/atproto-did` through `divine-router`.
- The PDS canary must pass before the user-facing opt-in flow is considered healthy.
- The bridge must not publish while `crosspost_enabled = false`, even if a DID already exists.
- Client feature flags must be required to expose the ATProto controls on mobile and web.
