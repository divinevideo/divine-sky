# ATProto Opt-In Smoke Test

Use this checklist to validate the end-to-end ATProto opt-in flow.

## Happy Path

1. Create or log in to a user in keycast.
2. Claim `username.divine.video`.
3. Verify `https://divine.video/.well-known/nostr.json?name=username` or the equivalent subdomain NIP-05 response.
4. Confirm ATProto is still disabled immediately after claim.
   - expected: `enabled = false`
   - expected: `state = null`
5. Enable ATProto from the authenticated client surface.
6. Verify keycast status returns `pending`.
7. Verify `divine-sky` provisions a `did:plc:...` and persists lifecycle `ready`.
8. Verify `divine-name-server` publishes `atproto_did` and `atproto_state = ready`.
9. Verify `divine-router` serves `https://username.divine.video/.well-known/atproto-did` and returns the bare DID.
10. Publish a Nostr video for the opted-in user.
11. Verify the mirrored ATProto post exists.
12. Disable ATProto.
13. Verify future mirrored posts stop and `divine-router` returns `404` for `/.well-known/atproto-did`.

## Failure Checks

- Username claim must not auto-enable ATProto.
- `pending`, `failed`, and `disabled` users must not resolve `/.well-known/atproto-did` through `divine-router`.
- The bridge must not publish while `crosspost_enabled = false`, even if a DID already exists.
- Client feature flags must be required to expose the ATProto controls on mobile and web.
