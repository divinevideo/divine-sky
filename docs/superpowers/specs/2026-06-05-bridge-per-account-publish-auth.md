# Bridge per-account publish auth (Wall 4) — Design Spec

**Date:** 2026-06-05
**Status:** Proposed — needs maintainer decision on the session-management approach before implementation.

## Problem (evidence-backed)

The bridge publishes every account's records (`com.atproto.repo.createRecord`, `uploadBlob`, `putRecord`, `deleteRecord`) using a **single shared Bearer token** — `config.pds_auth_token` (`PDS_AUTH_TOKEN`), passed once to `PdsClient::new(pds_url, pds_auth_token)` in `runtime.rs:478/487/495`.

This cannot work. rsky-pds enforces **per-DID auth** on repo writes:

- `rsky-pds/src/apis/com/atproto/repo/create_record.rs:50-51`:
  ```rust
  let did = account.did;
  if did != auth.access.credentials.unwrap().did.unwrap() {
      bail!("AuthRequiredError")
  }
  ```
  The authenticated DID **must equal** the repo being written to. One shared token can write to at most one DID's repo (its own), not every crossposting account's.
- Same file, line 47: `if account.deactivated_at.is_some() { bail!("Account is deactivated") }` — independently re-confirms Wall 3 (the account must be activated first; handled by PR #13).

Compounding: staging `PDS_AUTH_TOKEN` is literally `placeholder-token` — not a valid credential at all.

**Consequence:** even with provisioning fixed (PRs #12, #13), the publish step will fail `AuthRequiredError` for every account. No crosspost can land until the bridge authenticates **as each account**.

## What we already have

The Wall 3 fix (PR #13) makes `createAccount` return — and the bridge receive — an `accessJwt` + `refreshJwt` **for the account** at provisioning time. That session is exactly the credential repo writes require. Today it's used once (to activate) and discarded.

## Options

### Option A — Persist and use the per-account session (recommended)
- At provisioning, store the account's `accessJwt`/`refreshJwt` (e.g. a `pds_sessions` table keyed by DID, or columns on `account_links`).
- The publisher selects the session for the target DID and sends `Authorization: Bearer <that account's accessJwt>`.
- Refresh via `com.atproto.server.refreshSession` when the access token expires (401 → refresh → retry once); persist the rotated tokens.
- For accounts provisioned before this lands (the existing repos), mint a session via admin (`com.atproto.admin`/service auth) or `createSession` — needs the account password, which the bridge does not currently set/store (see open questions).

**Pros:** matches the ATProto model exactly; each write is correctly scoped. **Cons:** session storage + refresh lifecycle; backfill for existing accounts.

### Option B — Service-auth (inter-service JWT) signed by the bridge
- If rsky-pds trusts a service JWT (the `entryway`/service-DID mechanism the video path uses — cf. `memory: rsky-pds-video-upload-blocker`, where getServiceAuth with `aud=PDS service DID`, `lxm=...` worked), the bridge could mint a short-lived service JWT scoped per request instead of storing sessions.
- **Unknown:** whether rsky's `AccessStandardIncludeChecks` verifier accepts a service JWT as the repo DID for `createRecord`. Needs verification against `auth_verifier.rs`.

**Pros:** no session storage. **Cons:** depends on rsky accepting service JWTs for repo writes — must be confirmed, not assumed.

### Option C — Admin/superuser write bypass
- Some PDS impls allow an admin token to write any repo. rsky's `create_record` uses `AccessStandardIncludeChecks` (not admin), and the explicit `did != credentials.did` check suggests **no** such bypass exists. Likely a dead end; record and move on unless the verifier says otherwise.

## Recommendation

**Option A**, unless verifying `auth_verifier.rs` shows Option B (service JWT) is accepted for repo writes — in which case B is less state to manage. The decision hinges on one question answerable from rsky source: *what credential types does `AccessStandardIncludeChecks` accept, and does any satisfy `credentials.did == <arbitrary repo did>`?*

## Open questions for the maintainer
1. Does the bridge set an account **password** at `createAccount`? (Today it sends only `{did, handle}` — no password. `createSession`/refresh may need one. If not, sessions must come from the `createAccount` response only, and there's no recovery path if they're lost → leans toward storing refreshJwt durably.)
2. Is there an existing service-auth path (entryway/service DID) rsky-pds will accept for third-party repo writes? (If yes → Option B.)
3. How should the ~33 already-provisioned staging repos (and any prod ones) be backfilled with sessions?

## Acceptance criteria
- A crosspost `createRecord` for account X authenticates as DID X and succeeds against rsky-pds (no `AuthRequiredError`).
- Access-token expiry is handled (refresh + retry) without dropping the publish.
- Verified end-to-end: a real Nostr video from a `ready`+`crosspost_enabled` account appears AND plays on Bluesky.

## Not doing yet / why
Not implementing blind: this changes the publish hot path and the data model (session storage), and the right shape depends on the maintainer answers + one more rsky `auth_verifier.rs` read. The bug and the constraint are now precisely characterized; implementation should follow the decision.
