# login.divine.video

## Purpose

`login.divine.video` is the control-plane surface for DiVine account linking. It owns consent, provisioning triggers, status inspection, disable actions, export, and host-based `/.well-known/atproto-did` responses for `username.divine.video`.

## Route Responsibilities

- `POST /api/account-links/opt-in`
  Records a pending link after the user consents to ATProto mirroring.
- `POST /api/account-links/provision`
  Triggers or records provisioning completion for a linked account.
- `GET /api/account-links/:nostr_pubkey/status`
  Returns the current lifecycle state for the linked account.
- `POST /api/account-links/:nostr_pubkey/disable`
  Disables the link and makes handle resolution stop returning a DID.
- `GET /api/account-links/:nostr_pubkey/export`
  Returns the stored control-plane view of the linked account.
- `GET /.well-known/atproto-did`
  Resolves the current host, such as `alice.divine.video`, to a ready ATProto DID.

## State Contract

The control plane should track the same lifecycle states as provisioning:

- `pending`
- `ready`
- `failed`
- `disabled`

At minimum, each record needs `nostr_pubkey`, `handle`, `did`, `provisioning_state`, `provisioning_error`, `disabled_at`, `created_at`, and `updated_at`.

## Auth Assumptions

- Opt-in, provision, disable, and export routes must sit behind DiVine-authenticated sessions.
- `/.well-known/atproto-did` is public and host-based.
- Provisioning should be initiated only for a user who has already opted in.

## Operational Boundary

`login.divine.video` is a control plane, not a PDS:

- It decides whether a link exists and whether it is active.
- It coordinates with the bridge/provisioner to create or recover ATProto accounts.
- It serves host-based DID resolution once a link is `ready`.
- It must stop DID resolution immediately when a link is `disabled`.

The bridge and PDS remain responsible for publishing records, storing blobs, and serving ATProto APIs. `login.divine.video` should hand those concerns off rather than embedding them directly.

## Runtime Handoff

When a link reaches `ready`, the bridge runtime consumes that state through the shared `account_links` lifecycle contract. Disabling a link must prevent future handle resolution and must cause the bridge to skip further publish attempts for that pubkey.

For launch, treat `login.divine.video` and the bridge as one operational chain:

- control plane writes consent and provisioning state
- bridge reads ready state and relay offsets
- PDS executes blob and record writes with the configured auth token
