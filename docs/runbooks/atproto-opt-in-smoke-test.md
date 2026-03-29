# ATProto Opt-In Smoke Test

Use this checklist to validate the end-to-end ATProto opt-in flow across `rsky-pds`, `keycast`, `divine-sky`, `divine-name-server`, Fastly KV, and `divine-router`.

The production login contract is now separate from the lifecycle smoke:

- `username.divine.video` remains the public handle host
- `login.divine.video` remains the human console
- `pds.divine.video` is the production PDS host that must appear in user DID documents
- `entryway.divine.video` is the ATProto Authorization Server

Use `scripts/smoke-divine-atproto-login.sh` to validate the handle, DID, PDS, and entryway chain before running the opt-in lifecycle checks below.

Use the fast local stack in `config/docker-compose.yml` for bridge-only checks. Use the full `deploy/localnet/` lab when you need PLC, `divine.test` handle resolution, or a provisioning path that looks more like the real network surface.

The user-facing opt-in path still depends on sibling repos:

- `../keycast` for the consent UI and `/api/user/atproto/*`
- `../divine-name-server` for username state publication
- `../divine-router` for read-only `/.well-known/atproto-did`

`divine.test` is the local lab handle suffix. Public staging and production handles stay on `divine.video`.

## Preflight

1. Run the production login-chain smoke first:
   ```bash
   bash scripts/smoke-divine-atproto-login.sh
   ```
2. Verify `curl -fsS https://pds.divine.video/xrpc/com.atproto.server.describeServer` returns JSON.
3. Verify `curl -fsS https://entryway.divine.video/.well-known/oauth-authorization-server` returns JSON.
4. If you are testing a lab environment instead of production, use the localnet helper and the `divine.test` suffix.

For a staging or localnet PDS canary:
   ```bash
   PDS_URL=https://pds.staging.dvines.org \
   PDS_ADMIN_PASSWORD=... \
   CANARY_HANDLE=atproto-canary-$(date +%s).staging.dvines.org \
   CANARY_DID=did:plc:... \
   bash scripts/staging-pds-did-smoke.sh
   ```
5. Verify `curl -fsS https://pds.staging.dvines.org/xrpc/_health` returns `200`.
6. Verify `curl -fsS https://login.staging.dvines.org/api/user/atproto/status` is reachable.

For a localnet run instead of staging:

1. Start the lab slices under `deploy/localnet/`.
2. Load `deploy/localnet/bridge.env.example` and `deploy/localnet/handle-gateway.env.example` into the local `divine-atbridge` and `divine-handle-gateway` processes.
3. Claim and test `username.divine.test`, not `username.divine.video`.

## Happy Path

1. Create or log in to a verified cookie-auth user in `login.divine.video`.
2. Open `settings/security`.
3. In the `Bluesky Account` card, claim `username.divine.video` if the user does not already have one.
4. Verify the card shows the claimed handle preview and does not auto-enable ATProto.
   - expected UI: `Enable Bluesky account`
   - expected status: `enabled = false`
   - expected status: `state = null`
5. Verify `https://divine.video/.well-known/nostr.json?name=username` or the equivalent subdomain NIP-05 response.
6. Click `Enable Bluesky account` from the settings page.
7. Verify the card reaches `pending`.
   - expected UI: provisioning in progress
   - expected status: `enabled = true`
   - expected status: `state = pending`
8. Wait for the same user to reach `ready`.
   - expected UI: `@username.divine.video`
   - expected UI: visible `did:plc:...`
   - keycast status endpoint shows `state = ready`
   - `divine-sky` `account_links` shows `pending -> ready`
9. Inspect the `divine-name-server` D1 row for the username.
   - expected: `atproto_did = did:plc:...`
   - expected: `atproto_state = ready`
10. Inspect the Fastly KV record for `user:<username>`.
   - expected payload fields:
     - `status = active`
     - `atproto_did = did:plc:...`
     - `atproto_state = ready`
11. Verify `divine-router` serves `https://username.divine.video/.well-known/atproto-did` and returns the bare DID.
12. Publish a Nostr video for the opted-in user.
13. Verify the mirrored ATProto post exists after the user is `ready`.
14. Return to `settings/security` and click `Disable Bluesky account`.
15. Verify the card reaches `disabled`.
   - expected UI: public DID resolution and future cross-posting are disabled
   - expected status: `enabled = false`
   - expected status: `state = disabled`
16. Verify future mirrored posts stop and `divine-router` returns `404` for `/.well-known/atproto-did`.

## Failure Checks

- Username claim must not auto-enable ATProto.
- `pending`, `failed`, and `disabled` users must not resolve `/.well-known/atproto-did` through `divine-router`.
- `failed` must show the last provisioning error in the `Bluesky Account` card and keep a retry path on the same page.
- The PDS canary must pass before the user-facing opt-in flow is considered healthy.
- The production login contract must pass before production opt-in checks are treated as valid.
- The bridge must not publish while `crosspost_enabled = false`, even if a DID already exists.
- Client feature flags must be required to expose the ATProto controls on mobile and web.
